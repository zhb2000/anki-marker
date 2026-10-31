//! macOS 取词实现：优先辅助功能 API（AXUIElement），失败回退 AppleScript 模拟 Cmd+C。
//!
//! 实现参考了 yetone/get-selected-text（MIT / Apache-2.0）的 macOS 部分：
//! 优先通过辅助功能 API（AXUIElement）直接读取选中文字；
//! 目标应用不兼容时回退为执行 AppleScript——临时静音系统提示音、模拟按下
//! Cmd+C 读取剪贴板后恢复剪贴板与音量。“不兼容”包括两种情况：AX 调用报错，
//! 以及 AX “成功”但返回空串（如 VSCode 的 Monaco 编辑器用 canvas 自绘文本、
//! 另挂隐藏 textarea 接收输入，AXSelectedText 恒为空，见
//! docs/selected-text-research.local.md）。
//! 两条路径均要求在“系统设置 → 隐私与安全性 → 辅助功能”中授权本应用。
//!
//! 取句的几类提供方：标准 AX 文本元素（AXSelectedTextRange + AXStringForRange）、
//! text marker 提供方（AXTextMarker，如 Safari 网页、Chromium 系，见 marker_selected_context）、
//! PDFKit 文档（Preview、
//! Safari 的 PDF 查看器：选区位置不可得，经整页文本匹配定位，见
//! find_sentence_in_pdf_pages）。
//!
//! 回退 Cmd+C 后“剪贴板未变”的语义按 AX 路径分流（借鉴 Windows 端）：AX 能正常
//! 响应（返回空串）= 没读到选区，返回空串由调用方提示“没有选区”；AX 查询本身报错
//! 且剪贴板不变 = 无法证明目标里有选区，报取词失败。每次取词都会记日志：取词目标
//! 的 pid/进程名（`capture target`，目标是本应用自己时会特别标明）与最终结果预览。
//!
//! 选词取句（word-to-sentence）开启时入口为 get_selected_context：
//! AX 先读选中的词，再经 AXSelectedTextRange / AXStringForRange 读取词所在的
//! 上下文窗口并切出整个句子；选区横跨多个句子时（用户手动跨句/跨行选择）
//! 原样录入所选文本——只扩展、不丢弃；取句链路任一环节失败时降级为仅录入
//! 词原文，与既有的 AX 成功路径语义一致。

mod applescript;
mod ax;
mod ax_ffi;
mod marker;
mod pdf;
mod range;
mod retry;
mod text;

use super::capture::{describe_context, SentenceCapture};
use super::{RetryCallbacks, SelectedContext};
use applescript::get_selected_text_by_applescript;
use ax::{
    copy_string_attribute_by_ax, find_element_with_selection_by_ax, frontmost_application_pid,
    get_element_pid, get_focused_ui_element_by_ax,
};
use marker::{marker_selected_context, marker_selected_text, refine_truncated_with_markers};
use pdf::{find_sentence_in_pdf_pages, refine_with_pdf_pages};
use range::find_sentence_for_word_by_ax;
use retry::spawn_selection_retry;
use text::{looks_truncated, strip_text_artifacts, strip_text_artifacts_with_offset};

pub fn get_selected_text() -> Result<String, String> {
    match get_selected_text_by_ax() {
        Ok(text) if !text.trim().is_empty() => return Ok(text),
        Ok(_) => {
            // AX “成功”但为空：可能是真的没选中文本，也可能是目标应用不通过 AX
            // 暴露选区（如 VSCode），无法区分，统一回退到模拟 Cmd+C 再判一次；
            // AX 能正常响应，剪贴板不变即视为没选中——返回空串由调用方提示
            return get_selected_text_by_applescript(true);
        }
        Err(error) => {
            // AX 查询本身失败：剪贴板不变不能说明“没选中”（取词目标可能取错了、
            // 焦点在安全输入框、应用不理会模拟按键），按取词失败报错
            log::warn!("AX path failed, falling back to AppleScript: {error}");
            return get_selected_text_by_applescript(false);
        }
    }
}

/// word_to_sentence 为 true 时尝试“选词取句”：AX 全链路成功返回 {句子, 词}；
/// AX 拿到词但取句失败返回 {词原文, None}；AX 报错或词为空时回退 AppleScript（同现有逻辑）。
/// word_to_sentence 为 false 时完全等同现有 get_selected_text 的行为。
///
/// `callbacks`：AX 失败回退后，若目标应用的无障碍树在后台异步物化（如 Word），
/// 后台重试取到完整句子时经 `on_finished` 补发结果；`on_started`/`on_finished`
/// 同时供 UI 呈现“仍在补取”状态（见 RetryCallbacks 的调用约定）。
pub fn get_selected_context(
    word_to_sentence: bool,
    callbacks: RetryCallbacks,
) -> Result<SelectedContext, String> {
    if !word_to_sentence {
        let context = get_selected_text().map(|text| SelectedContext {
            text,
            word: None,
            word_offset: None,
        })?;
        log::info!(
            "capture (sentence mode off): {}",
            describe_context(&context)
        );
        return Ok(context);
    }
    // AX 报错（true）与 AX 正常但选区为空（false）在回退 Cmd+C 后对“剪贴板未变”
    // 的解读不同，见 get_selected_text_by_applescript 的 silent_if_unchanged
    // AX 报错时的补全目标：模拟复制只拿到词后，尝试用目标应用的 PDF 页面文本取句
    let mut pdf_refine_pid: Option<i32> = None;
    let silent_if_unchanged = match get_selected_context_by_ax() {
        Ok(context) if !context.text.trim().is_empty() => {
            log::info!("capture via AX: {}", describe_context(&context));
            return Ok(context);
        }
        Ok(_) => {
            // AX “成功”但为空：无法区分“真没选”与“应用不暴露选区”（如 VSCode 的
            // Monaco 编辑器），回退模拟 Cmd+C 再判一次；记日志便于诊断各应用的 AX 暴露情况
            log::warn!(
                "AX path returned empty selected text (no selection or the app does not \
                 expose its selection via AX), falling back to AppleScript"
            );
            true
        }
        Err(ax_error) => {
            // AX 失败（含树搜索无果）：立即回退 AppleScript 保证录入不阻塞；
            // 若拿到了目标应用 pid，同时启动后台重试——无障碍树异步物化后补发句子
            if let Some(pid) = ax_error.retry_pid {
                log::info!("spawning background AX selection retry for pid {pid}");
                spawn_selection_retry(pid, callbacks);
            }
            log::warn!(
                "AX path failed, falling back to AppleScript: {}",
                ax_error.message
            );
            pdf_refine_pid = ax_error.retry_pid.or_else(frontmost_application_pid);
            false
        }
    };
    let mut context =
        get_selected_text_by_applescript(silent_if_unchanged).map(|text| SelectedContext {
            text,
            word: None,
            word_offset: None,
        })?;
    if let Some(pid) = pdf_refine_pid {
        if !context.text.trim().is_empty() {
            if let Some(refined) = refine_with_pdf_pages(pid, &context.text) {
                context = refined;
            }
        }
    }
    log::info!(
        "capture via simulated Cmd+C: {}",
        describe_context(&context)
    );
    return Ok(context);
}

/// 通过辅助功能 API 读取当前焦点元素中的选中文本。
///
/// 注意：未授予辅助功能权限时此处会失败，由调用方回退或提示。
fn get_selected_text_by_ax() -> Result<String, String> {
    let focused = get_focused_ui_element_by_ax()?;
    return match copy_string_attribute_by_ax(&focused, ax_ffi::SELECTED_TEXT_ATTRIBUTE) {
        Ok(text) => Ok(text),
        // text marker 提供方（Safari 网页等）不提供 AXSelectedText，改读 marker 范围
        Err(error) => match marker_selected_text(&focused) {
            Some(text) => Ok(text),
            None => Err(error),
        },
    };
}

/// AX 取句链路的失败：携带诊断信息与（可能存在的）后台重试句柄（目标应用 pid）。
#[derive(Debug)]
struct AxAttemptError {
    message: String,
    retry_pid: Option<i32>,
}

/// 通过辅助功能 API 读取选中的词及其所在的句子（选词取句的 AX 链路）。
///
/// 成功取句返回 {整句, Some(词)}；用户手动跨句选择时返回 {所选原文, None}；
/// 拿到词但取句链路任一环节失败时降级返回 {词原文, None}（语义等同现有的
/// AX 成功路径，不再回退 AppleScript）；连词都取不到时才返回 Err（携带
/// 后台重试句柄——部分应用的无障碍树在被查询后异步物化，如 Word）。
fn get_selected_context_by_ax() -> Result<SelectedContext, AxAttemptError> {
    let focused = get_focused_ui_element_by_ax().map_err(|error| AxAttemptError {
        message: error,
        retry_pid: None,
    })?;
    let word = match copy_string_attribute_by_ax(&focused, ax_ffi::SELECTED_TEXT_ATTRIBUTE) {
        Ok(word) => word,
        Err(focused_error) => {
            // text marker 提供方（Safari 网页、Chromium 系等）：选区只经 AXTextMarkerRange
            // 暴露，先走 marker 链路。命中即返回——含“确认是此类提供方但没有选区”的空结果，避免对没选中的
            // 常见操作白白做树搜索与后台重试
            if let Some(context) = marker_selected_context(&focused) {
                return Ok(context);
            }
            // 焦点元素读不到选区（如 Zotero 笔记编辑器：kAXErrorNoValue，焦点落在
            // 容器上）：立即做一次树搜索（部分应用首次即可命中）；无果则把目标
            // 应用 pid 交给后台重试——无障碍树可能异步物化，实测 Word 第二次划词
            // 才成功，后台轮询可在同一次按键内补上完整句子
            match find_element_with_selection_by_ax(&focused) {
                Ok(Some((element, word))) => {
                    log::info!(
                        "found the selection via AX tree search \
                         (focused element error was: {focused_error})"
                    );
                    return Ok(context_from_word_and_element(&element, &word));
                }
                _ => {
                    let retry_pid = get_element_pid(&focused);
                    return Err(AxAttemptError {
                        message: format!(
                            "{focused_error}; AX tree search found no element with a selection"
                        ),
                        retry_pid,
                    });
                }
            }
        }
    };
    if word.trim().is_empty() {
        // 与现有 get_selected_text 的空串语义一致：由调用方回退 AppleScript
        return Ok(SelectedContext {
            text: word,
            word: None,
            word_offset: None,
        });
    }
    return Ok(context_from_word_and_element(&focused, &word));
}

/// 拿到所选文本后的收尾：对持有选区的元素跑取句链路，失败降级为仅录词原文。
pub(super) fn context_from_word_and_element(
    element: &core_foundation::base::CFType,
    word: &str,
) -> SelectedContext {
    let context = match find_sentence_for_word_by_ax(element, word) {
        Ok(SentenceCapture::Expanded {
            sentence,
            word_offset,
        }) => {
            // 范围链路的上下文可能带视觉行硬换行（Chrome 的 PDF 查看器：同时提供
            // AXSelectedText 与 marker 范围，但 AXStringForRange 的文本按行给出），
            // UAX #29 在换行处强制分句，句子被切成碎片。看起来被截断、且元素支持
            // marker 范围时，改用 marker 链路（带行扩展与抹换行）重试
            if looks_truncated(&sentence) {
                if let Some(refined) = refine_truncated_with_markers(element, word) {
                    return refined;
                }
            }
            SelectedContext {
                text: sentence,
                word: Some(word.trim().to_string()),
                word_offset,
            }
        }
        Ok(SentenceCapture::SelectionAsIs(text)) => SelectedContext {
            text,
            word: None,
            word_offset: None,
        },
        Err(error) => {
            // 范围链路失败。PDFKit（Preview、Safari 的 PDF 查看器）不暴露选区位置，
            // 尝试用整页文本匹配选中词来定位；仍失败则降级为现有 AX 成功路径的语义
            // （录入词原文）——已拿到词，取句失败不致命
            match find_sentence_in_pdf_pages(element, word) {
                Ok(context) => context,
                Err(pdf_error) => {
                    log::warn!(
                        "failed to extract sentence for the selected word: {error} \
                         (PDF page lookup: {pdf_error})"
                    );
                    SelectedContext {
                        text: word.to_string(),
                        word: None,
                        word_offset: None,
                    }
                }
            }
        }
    };
    let (text, word_offset) = strip_text_artifacts_with_offset(&context.text, context.word_offset);
    return SelectedContext {
        text,
        word: context.word.map(|word| strip_text_artifacts(&word)),
        word_offset,
    };
}

#[cfg(test)]
mod tests {
    use super::{get_selected_context_by_ax, get_selected_text_by_ax};

    /// 冒烟测试：FFI 调用路径不应 panic，结果取决于辅助功能权限与当前焦点元素。
    #[test]
    fn ax_ffi_smoke_test() {
        let result = get_selected_text_by_ax();
        eprintln!("get_selected_text_by_ax() -> {result:?}");
    }

    /// 冒烟测试：取句 AX 链路不应 panic，结果取决于辅助功能权限与当前焦点元素。
    /// 只测 AX 路径：完整的 get_selected_context 会回退 AppleScript 模拟 Cmd+C 按键，
    /// 不适合在测试中运行。
    #[test]
    fn selected_context_by_ax_smoke_test() {
        let result = get_selected_context_by_ax();
        eprintln!("get_selected_context_by_ax() -> {result:?}");
    }
}
