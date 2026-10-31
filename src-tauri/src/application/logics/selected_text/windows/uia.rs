//! UI Automation（TextPattern）取词：COM 初始化守卫、读取焦点元素或指定窗口内的选区，
//! 以及以选区为锚点扩展上下文窗口并切句。

use super::super::capture::{capture_in_context, SentenceCapture};
use super::super::SelectedContext;

use ::windows::core::BSTR;
use ::windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize};
use ::windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use ::windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextRange,
    TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Character,
    UIA_TextPatternId,
};

/// COM 生命周期守卫：Drop 时 CoUninitialize。守卫先于所有 COM 接口构造、
/// 后于所有 COM 接口析构（声明顺序在前 → drop 顺序在后），保证 UIA 接口
/// 全部释放后再反初始化 COM。
pub(super) struct ComGuard {
    _private: (),
}

impl ComGuard {
    /// 在调用线程上以 MTA 初始化 COM（UIA 客户端的推荐线程模型；本应用的取词
    /// 链路全部在独立线程上执行，天然满足“不拥有窗口的独立线程”要求）。
    /// S_OK/S_FALSE（已初始化）均视为成功，CoUninitialize 与之配对平衡。
    pub(super) fn init() -> Result<ComGuard, String> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            return Ok(ComGuard { _private: () });
        }
        return Err(format!("failed to initialize COM: {result}"));
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// 创建 UIA 客户端实例（CUIAutomation 进程内 COM 服务器）。
pub(super) fn uia_automation() -> Result<IUIAutomation, String> {
    return unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
        .map_err(|error| format!("failed to create CUIAutomation: {error}"));
}

/// 读取当前持有选区的 TextRange：焦点元素 → TextPattern → 当前选区。
fn uia_selection(automation: &IUIAutomation) -> Result<IUIAutomationTextRange, String> {
    let element = unsafe { automation.GetFocusedElement() }
        .map_err(|error| format!("failed to get the focused element: {error}"))?;
    let pattern: IUIAutomationTextPattern =
        unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) }.map_err(|error| {
            // 控件不支持某 pattern 时 UIA 返回 S_OK + 空对象，windows-rs 将空对象转错误时
            // 捡到线程上残留的错误码，报出“操作成功完成 (0x00000000)”这类误导性信息——
            // 归一化为明确的语义（典型场景：Chromium 的无障碍树尚未被 UIA 客户端激活）
            if error.code() == ::windows::core::HRESULT(0) {
                "the focused element does not support TextPattern (returned null; the app's \
                 accessibility tree may not be materialized yet)"
                    .to_string()
            } else {
                format!("the focused element does not support TextPattern: {error}")
            }
        })?;
    return text_pattern_selection(&pattern);
}

/// 从 TextPattern 取当前选区（第一个非空选区范围）。
fn text_pattern_selection(
    pattern: &IUIAutomationTextPattern,
) -> Result<IUIAutomationTextRange, String> {
    let ranges = unsafe { pattern.GetSelection() }
        .map_err(|error| format!("failed to get the text selection: {error}"))?;
    let length = unsafe { ranges.Length() }
        .map_err(|error| format!("failed to get the selection range count: {error}"))?;
    if length <= 0 {
        return Err("the element has no text selection".to_string());
    }
    return unsafe { ranges.GetElement(0) }
        .map_err(|error| format!("failed to get the first selection range: {error}"));
}

/// 在指定窗口内找持有非空选区的 TextRange（后台重试用，不跟随系统焦点——
/// 失败/成功弹窗会夺走焦点，跟随焦点会把查询打到我们自己的窗口上）：
/// 窗口元素 → 后代中所有支持 TextPattern 的元素 → 第一个持有非空选区的。
pub(super) fn uia_selection_in_window(
    automation: &IUIAutomation,
    hwnd: ::windows::Win32::Foundation::HWND,
) -> Result<IUIAutomationTextRange, String> {
    use ::windows::Win32::UI::Accessibility::{
        TreeScope_Descendants, UIA_IsTextPatternAvailablePropertyId,
    };

    let element = unsafe { automation.ElementFromHandle(hwnd) }
        .map_err(|error| format!("failed to get the element for the window: {error}"))?;
    let condition = unsafe {
        automation.CreatePropertyCondition(UIA_IsTextPatternAvailablePropertyId, &variant_true())
    }
    .map_err(|error| format!("failed to create the TextPattern property condition: {error}"))?;
    let candidates =
        unsafe { element.FindAll(TreeScope_Descendants, &condition) }.map_err(|error| {
            format!("failed to search the window for TextPattern elements: {error}")
        })?;
    // 遍历上限：防御病态提供方返回巨量元素（正常窗口只有少数文本控件）
    let count = unsafe { candidates.Length() }.unwrap_or(0).min(64);
    for index in 0..count {
        let Ok(candidate) = (unsafe { candidates.GetElement(index) }) else {
            continue;
        };
        let Ok(pattern) = (unsafe {
            candidate.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        }) else {
            continue;
        };
        let Ok(range) = text_pattern_selection(&pattern) else {
            continue;
        };
        let non_empty = unsafe { range.GetText(-1) }
            .ok()
            .and_then(|text| bstr_to_string(&text).ok())
            .is_some_and(|text| !text.trim().is_empty());
        if non_empty {
            return Ok(range);
        }
    }
    return Err("no element with a text selection found in the window".to_string());
}

/// 构造 VT_BOOL = true 的 VARIANT（windows crate 未提供 From<bool> 转换）。
fn variant_true() -> ::windows::Win32::System::Variant::VARIANT {
    use ::windows::Win32::Foundation::VARIANT_BOOL;
    use ::windows::Win32::System::Variant::{
        VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL,
    };

    return VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_BOOL,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 {
                    boolVal: VARIANT_BOOL(-1),
                },
            }),
        },
    };
}

/// 通过 UIA 读取当前焦点控件中的选中文本。
pub(super) fn uia_selected_text() -> Result<String, String> {
    let _com = ComGuard::init()?;
    let range = uia_selection(&uia_automation()?)?;
    let text = unsafe { range.GetText(-1) }
        .map_err(|error| format!("failed to get the selected text: {error}"))?;
    return bstr_to_string(&text);
}

/// 通过 UIA 读取选中的词及其所在的句子（选词取句的 UIA 链路）。
///
/// 拿到词但取句链路失败时降级返回 {词原文, None}；连词都取不到时才返回 Err。
pub(super) fn uia_selected_context() -> Result<SelectedContext, String> {
    let _com = ComGuard::init()?;
    let range = uia_selection(&uia_automation()?)?;
    return context_from_range(&range);
}

/// 拿到持有选区的 TextRange 后的收尾：读词 → 取句链路 → 失败降级为仅录词原文。
pub(super) fn context_from_range(
    range: &IUIAutomationTextRange,
) -> Result<SelectedContext, String> {
    let word = bstr_to_string(
        &unsafe { range.GetText(-1) }
            .map_err(|error| format!("failed to get the selected text: {error}"))?,
    )?;
    if word.trim().is_empty() {
        // 与 get_selected_text 的空串语义一致：由调用方回退 Ctrl+C
        return Ok(SelectedContext {
            text: word,
            word: None,
            word_offset: None,
        });
    }
    return Ok(match find_sentence_for_word(range, &word) {
        Ok(SentenceCapture::Expanded {
            sentence,
            word_offset,
        }) => SelectedContext {
            text: sentence,
            word: Some(word.trim().to_string()),
            word_offset,
        },
        Ok(SentenceCapture::SelectionAsIs(text)) => SelectedContext {
            text,
            word: None,
            word_offset: None,
        },
        Err(error) => {
            // 已拿到词：取句失败不致命，降级为仅录入词原文
            log::warn!("failed to extract sentence for the selected word: {error}");
            SelectedContext {
                text: word,
                word: None,
                word_offset: None,
            }
        }
    });
}

/// 上下文窗口的单侧余量（字符数）。UIA 的 MoveEndpointByUnit 越界自动截停到
/// 文本边缘而非报错，天然替代了 macOS 侧的字符数夹取与右缘收缩重试；
/// 窗口、前缀、选区文本均经同一接口的 GetText 读取，坐标系自洽。
const WINDOW_MARGIN: i32 = 2048;

/// 取句链路：以选区为中心扩出上下文窗口 → 用前缀范围推导选区的 UTF-16 偏移
/// → 共享 capture 模块校验锚点并切句。任一环节失败返回 Err，由调用方降级。
fn find_sentence_for_word(
    selection: &IUIAutomationTextRange,
    word: &str,
) -> Result<SentenceCapture, String> {
    // 上下文窗口：选区向两侧各扩 WINDOW_MARGIN 个字符
    let window = unsafe { selection.Clone() }
        .map_err(|error| format!("failed to clone the selection range: {error}"))?;
    let moved_left = unsafe {
        window.MoveEndpointByUnit(
            TextPatternRangeEndpoint_Start,
            TextUnit_Character,
            -WINDOW_MARGIN,
        )
    }
    .map_err(|error| format!("failed to extend the context window to the left: {error}"))?;
    unsafe {
        window.MoveEndpointByUnit(
            TextPatternRangeEndpoint_End,
            TextUnit_Character,
            WINDOW_MARGIN,
        )
    }
    .map_err(|error| format!("failed to extend the context window to the right: {error}"))?;
    let context = bstr_to_string(
        &unsafe { window.GetText(-1) }
            .map_err(|error| format!("failed to read the context window: {error}"))?,
    )?;

    // 选区在窗口内的 UTF-16 偏移：前缀范围（窗口副本终点收缩到选区起点）的文本长度
    let prefix = unsafe { window.Clone() }
        .map_err(|error| format!("failed to clone the context window: {error}"))?;
    unsafe {
        prefix.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            selection,
            TextPatternRangeEndpoint_Start,
        )
    }
    .map_err(|error| {
        format!("failed to shrink the prefix range to the selection start: {error}")
    })?;
    // 硬校验：前缀终点必须恰好落在选区起点（MoveEndpointByRange 静默失灵的防线）。
    // 不过则整个取句链路作废、降级录词——宁可不取句，也不切错句子
    match unsafe {
        prefix.CompareEndpoints(
            TextPatternRangeEndpoint_End,
            selection,
            TextPatternRangeEndpoint_Start,
        )
    } {
        Ok(0) => {}
        Ok(offset) => {
            return Err(format!(
                "the prefix endpoint does not align with the selection start (off by {offset})"
            ));
        }
        Err(error) => return Err(format!("failed to compare range endpoints: {error}")),
    }
    let sel_start_utf16 = unsafe { prefix.GetText(-1) }
        .map_err(|error| format!("failed to read the prefix text: {error}"))?
        .len() as isize; // BSTR 按长度前缀解引用为 [u16]，len() 即 UTF-16 码元数

    // window_loc：窗口实际左移量（0 表示已对齐文本开头），供触边判定
    let window_loc = (-moved_left) as isize;
    // 诊断日志：定位取句错误（切错句子/降级）的实机证据
    log::info!(
        "UIA sentence capture: word {} utf16 units, selection starts at {}, \
         window_loc {}, context {} utf16 units, {} occurrence(s) of the word in context",
        word.encode_utf16().count(),
        sel_start_utf16,
        window_loc,
        context.encode_utf16().count(),
        context.match_indices(word).count()
    );
    // Windows 不用“唯一出现位置”兜底：推导偏移校验失败意味着前缀推导系统性失灵，
    // 兜底会把“定位失败”变成“切出错误句子”，此时降级录词（见 capture.rs find_anchor）
    match capture_in_context(&context, word, &[sel_start_utf16], window_loc, false) {
        Some((capture, touched_edge)) => {
            if touched_edge {
                // ±2048 字符的窗口对正常句子足够大，触边基本意味着超长文本块；
                // v1 不做更大窗口的重试，记日志观察
                log::info!(
                    "sentence touches the edge of a ±{WINDOW_MARGIN}-character context window \
                     (context {} utf16 units), accepting a possibly truncated sentence",
                    context.encode_utf16().count()
                );
            }
            return Ok(capture);
        }
        None => {
            return Err(format!(
                "no validated anchor for the selected text in the context \
                 (selection starts at {} utf16 units, context {} utf16 units, \
                 word {} utf16 units, {} occurrence(s) in context)",
                sel_start_utf16,
                context.encode_utf16().count(),
                word.encode_utf16().count(),
                context.match_indices(word).count()
            ));
        }
    }
}

/// BSTR（长度前缀的 UTF-16 宽串）转 String；含孤代理对时报错（不 panic、不截断）。
fn bstr_to_string(text: &BSTR) -> Result<String, String> {
    return String::try_from(text)
        .map_err(|error| format!("the text is not valid UTF-16: {error}"));
}

#[cfg(test)]
mod tests {
    /// 冒烟测试：UIA 调用路径不应 panic；无桌面环境（CI）下允许返回 Err。
    #[test]
    fn uia_smoke_test() {
        let result = super::uia_selected_text();
        eprintln!(
            "uia_selected_text() -> {:?}",
            result.map(|text| text.chars().count())
        );
    }
}
