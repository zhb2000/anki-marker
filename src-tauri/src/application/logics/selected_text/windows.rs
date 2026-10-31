//! Windows 取词实现：UI Automation（TextPattern）+ 模拟 Ctrl+C 剪贴板权威调和。
//!
//! 两条路径分工：UIA 提供选区偏移与上下文（剪贴板做不到）；模拟 Ctrl+C 读到的
//! 剪贴板内容是用户真实选区（UIA 做不到的权威——Edge PDF 的 Adobe 引擎文本层与
//! 视觉渲染错位，UIA 选区实测漂移 2 个字符）。因此 UIA 成功后仍经 Ctrl+C 调和
//! （reconcile_with_primary）：句子含权威词则替换词、不含则降级录权威文本。
//!
//! UIA 拿不到选区（控件不支持 TextPattern、GetSelection 为空或“成功”但返回空串，
//! 如自绘文本的编辑器）时回退纯 Ctrl+C。剪贴板序号不变的语义按 UIA 路径分流：
//! UIA 能正常响应（返回空串）= 没读到选区，返回空结果由调用方提示（无法区分
//! “真的没选”与“应用不向 UIA 暴露选区”）；UIA 报错（查询都失败）且剪贴板不变 =
//! 报取词失败。后者有两种成因：前台窗口以管理员身份运行（UIPI 拦截注入输入），
//! 或前台窗口本就是我们自己的窗口（托盘菜单抢占前台且未归还，见 windows_target
//! ——该情形已由 ensure_foreign_foreground 前置处理，走到这里说明归还也没成，
//! 会另记一条诊断日志）。
//!
//! Ctrl+C 只在“真复制”时才改动用户的剪贴板：注入前查前台窗口，终端里跳过注入（Ctrl+C
//! 在那里是中断信号，语义同“剪贴板不变”）；序号没变时不做任何恢复；复制发生后无论读取
//! 成败都恢复备份。备份覆盖全部可安全复制的格式（见 clipboard_backup），恢复时带上
//! 剪贴板历史排除标记。
//!
//! UIA 无需特殊权限。
//! 后台重试：Chromium 系应用在 UIA 客户端查询时才按需物化无障碍树，重试锁定
//! 目标窗口 HWND（不跟随焦点——弹窗会夺走焦点），树就绪后补发完整结果；
//! 目标窗口由 windows_target::retry_target 给出（前台优先，托盘菜单占着前台时
//! 退到那里跟踪的“用户最近所在的应用窗口”——见 windows_target）。
//!
//! ## 已知限制：UWP 应用取不到选区（2026-09-28 记录，待查）
//!
//! 实测一个 UWP 应用（窗口类 `ApplicationFrameWindow`，宿主 `ApplicationFrameHost.exe`）：
//! 快捷键触发时 UIA 报“焦点元素不支持 TextPattern”、注入的 Ctrl+C 未改变剪贴板；
//! 托盘菜单触发时（归还前台后）UIA 返回**空选区**——两条路都拿不到内容，且**无论是否
//! 选中文字表现一致**；后台重试（窗口内搜持有 TextPattern 的元素）扫 10 次也无结果。
//! 而**手工** Ctrl+C 能复制。待查方向：① 注入的按键可能没有进入 UWP 的输入通道；
//! ② 我们只把前台还给了 frame 窗口，应用内容窗口未必拿到键盘焦点（若属此项，可在归还
//! 后进一步把焦点落到内容子窗口）。两条都不影响非 UWP 应用。

use super::capture::{
    capture_in_context, describe_context, reconcile_with_primary, SentenceCapture,
};
use super::{RetryCallbacks, SelectedContext};

use ::windows::core::BSTR;
use ::windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize};
use ::windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use ::windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextRange,
    TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Character,
    UIA_TextPatternId,
};
use ::windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;

pub fn get_selected_text() -> Result<String, String> {
    match uia_selected_text() {
        Ok(text) if !text.trim().is_empty() => {
            // UIA 拿到文本：仍经剪贴板调和（选区漂移/撞错控件的修正；
            // 调和失败即剪贴板未变时原样返回 UIA 结果）
            return Ok(reconcile_with_primary(text, None, None, copy_via_ctrl_c(true).ok()).text);
        }
        Ok(_) => {
            // UIA “成功”但为空：无法区分“真没选”与“应用不暴露选区”（如 VSCode），
            // 回退 Ctrl+C 再判一次；应用对 UIA 响应正常，剪贴板不变即为真没选——静默
            log::warn!(
                "UIA path returned empty selected text (no selection or the app does not \
                 expose its selection via UIA), falling back to simulated Ctrl+C"
            );
            return copy_via_ctrl_c(true);
        }
        Err(error) => {
            log::warn!("UIA path failed, falling back to simulated Ctrl+C: {error}");
            return copy_via_ctrl_c(false);
        }
    }
}

/// word_to_sentence 为 true 时尝试“选词取句”：UIA 全链路成功后再经剪贴板调和
/// 返回 {句子, 权威词}；UIA 报错或词为空时回退模拟 Ctrl+C。
/// word_to_sentence 为 false 时完全等同 get_selected_text 的行为。
///
/// `callbacks`：UIA 失败回退后，若目标应用的无障碍树在后台物化
/// （Chromium 系按需激活），后台重试取到完整句子时经 `on_finished` 补发结果；
/// `on_started`/`on_finished` 同时供 UI 呈现“仍在补取”状态（见 RetryCallbacks
/// 的调用约定）。
pub fn get_selected_context(
    word_to_sentence: bool,
    callbacks: RetryCallbacks,
) -> Result<SelectedContext, String> {
    if !word_to_sentence {
        return get_selected_text().map(|text| SelectedContext {
            text,
            word: None,
            word_offset: None,
        });
    }
    match uia_selected_context() {
        Ok(context) if !context.text.trim().is_empty() => {
            let reconciled = reconcile_with_primary(
                context.text,
                context.word,
                context.word_offset,
                copy_via_ctrl_c(true).ok(),
            );
            log::info!("capture via UIA: {}", describe_context(&reconciled));
            return Ok(reconciled);
        }
        Ok(_) => {
            // UIA “成功”但为空：无法区分“真没选”与“应用不暴露选区”，回退 Ctrl+C
            // 再判一次（静默模式：剪贴板不变 = 真没选）；不触发后台重试，以免给
            // “未选中就按快捷键”的常见操作增加延迟
            log::warn!(
                "UIA path returned empty selected text (no selection or the app does not \
                 expose its selection via UIA), falling back to simulated Ctrl+C"
            );
            return copy_via_ctrl_c(true).map(|text| SelectedContext {
                text,
                word: None,
                word_offset: None,
            });
        }
        Err(error) => {
            // UIA 失败：立即回退 Ctrl+C 保证录入不阻塞（严格模式：剪贴板不变 =
            // 前台应用不理会注入的输入，多为 UIPI 提权窗口——报取词失败）；
            // 同时启动后台重试——Chromium 系应用的无障碍树在被 UIA 客户端查询后
            // 按需物化，树就绪后补发句子。
            // 重试锁定目标窗口的 HWND 而不是跟随焦点：随后的回退结果/失败提示
            // 会弹出本应用窗口夺走焦点，跟随焦点会把查询打到我们自己的窗口上；
            // 托盘菜单路径下前台还可能是我们自己的窗口（见 windows_target），
            // retry_target 优先取前台、否则退回跟踪到的用户应用窗口
            if super::windows_target::foreground_is_own() {
                // 诊断：此时 UIA 报的“focused element 不支持 TextPattern”指的很可能是
                // 我们自己的窗口（托盘菜单抢走前台且未归还），与目标应用无关
                log::warn!(
                    "the foreground window belongs to this process; the capture cannot reach the \
                     target app (the tray menu may have taken the foreground window)"
                );
            }
            if let Some(hwnd) = super::windows_target::retry_target() {
                log::info!("spawning background UIA selection retry");
                spawn_selection_retry(hwnd, callbacks);
            } else {
                log::info!("no capture target window, skipping the background UIA selection retry");
            }
            log::warn!("UIA path failed, falling back to simulated Ctrl+C: {error}");
            let context = copy_via_ctrl_c(false).map(|text| SelectedContext {
                text,
                word: None,
                word_offset: None,
            })?;
            log::info!(
                "capture via simulated Ctrl+C: {}",
                describe_context(&context)
            );
            return Ok(context);
        }
    }
}

/// COM 生命周期守卫：Drop 时 CoUninitialize。守卫先于所有 COM 接口构造、
/// 后于所有 COM 接口析构（声明顺序在前 → drop 顺序在后），保证 UIA 接口
/// 全部释放后再反初始化 COM。
struct ComGuard {
    _private: (),
}

impl ComGuard {
    /// 在调用线程上以 MTA 初始化 COM（UIA 客户端的推荐线程模型；本应用的取词
    /// 链路全部在独立线程上执行，天然满足“不拥有窗口的独立线程”要求）。
    /// S_OK/S_FALSE（已初始化）均视为成功，CoUninitialize 与之配对平衡。
    fn init() -> Result<ComGuard, String> {
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
fn uia_automation() -> Result<IUIAutomation, String> {
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
fn uia_selection_in_window(
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
fn uia_selected_text() -> Result<String, String> {
    let _com = ComGuard::init()?;
    let range = uia_selection(&uia_automation()?)?;
    let text = unsafe { range.GetText(-1) }
        .map_err(|error| format!("failed to get the selected text: {error}"))?;
    return bstr_to_string(&text);
}

/// 通过 UIA 读取选中的词及其所在的句子（选词取句的 UIA 链路）。
///
/// 拿到词但取句链路失败时降级返回 {词原文, None}；连词都取不到时才返回 Err。
fn uia_selected_context() -> Result<SelectedContext, String> {
    let _com = ComGuard::init()?;
    let range = uia_selection(&uia_automation()?)?;
    return context_from_range(&range);
}

/// 拿到持有选区的 TextRange 后的收尾：读词 → 取句链路 → 失败降级为仅录词原文。
fn context_from_range(range: &IUIAutomationTextRange) -> Result<SelectedContext, String> {
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

/// 后台重试的互斥标记：同一时间只允许一个重试在途（避免连按快捷键叠加轮询线程）。
static RETRYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 重试的收尾守卫：`RETRYING` 复位与 `on_finished` 恰好一次都收在这里。线程中途 panic
/// 时由 Drop 兜底（按超时放弃处理），否则互斥标记会卡死，此后再也不会启动重试。
struct RetryGuard {
    on_finished: Option<Box<dyn FnOnce(Option<SelectedContext>) + Send>>,
}

impl RetryGuard {
    fn finish(mut self, result: Option<SelectedContext>) {
        self.release(result);
    }

    fn release(&mut self, result: Option<SelectedContext>) {
        let Some(on_finished) = self.on_finished.take() else {
            return;
        };
        RETRYING.store(false, std::sync::atomic::Ordering::SeqCst);
        on_finished(result);
    }
}

impl Drop for RetryGuard {
    fn drop(&mut self) {
        self.release(None);
    }
}

/// 后台重试：Chromium 系应用在 UIA 客户端查询时才按需物化无障碍树，首次查询
/// 可能失败。按固定间隔轮询目标窗口（锁定 HWND 重跑取句链路，不跟随系统焦点——
/// 失败/成功弹窗会夺走焦点），取到非空选区后经 `on_finished` 补发完整结果；
/// 超时放弃或重试已过期（`is_cancelled`）时 `on_finished(None)`。
fn spawn_selection_retry(hwnd: ::windows::Win32::Foundation::HWND, callbacks: RetryCallbacks) {
    /// 轮询次数与间隔（总计约 6 秒，覆盖 Chromium 首次物化无障碍树的耗时）
    const RETRY_ATTEMPTS: usize = 10;
    const RETRY_INTERVAL_MS: u64 = 600;

    /// 轮询间隔内检查过期的粒度：旧重试据此在新一次取词后很快退出、让出互斥
    const CANCEL_CHECK_MS: u64 = 50;
    /// 等待旧重试让出互斥的上限（旧重试已随新一次取词过期，至多一个检查粒度加一次
    /// UIA 查询就会退出）
    const ACQUIRE_TIMEOUT_MS: u64 = 2000;

    // HWND 是窗口句柄（实为数值，不含可析构资源），可安全跨线程传递
    let hwnd_raw = hwnd.0 as usize;
    let RetryCallbacks {
        on_started,
        on_finished,
        is_cancelled,
    } = callbacks;
    std::thread::spawn(move || {
        // 同一时间只允许一个重试在途。取词本身已互斥，此时在途的重试必然属于上一次
        // 取词、已经过期，会很快退出——等它让出，而不是直接放弃（直接放弃会让新一次
        // 取词在重试窗口内再也补不回句子）。等待中自身过期或超时则不触发任何回调
        let start = std::time::Instant::now();
        while RETRYING.swap(true, std::sync::atomic::Ordering::SeqCst) {
            if is_cancelled() || start.elapsed().as_millis() >= ACQUIRE_TIMEOUT_MS as u128 {
                log::info!("UIA selection retry skipped: another retry is still running");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(CANCEL_CHECK_MS));
        }
        // 互斥标记一经占用就由守卫负责复位，并保证 on_finished 恰好一次（含 panic）
        let guard = RetryGuard {
            on_finished: Some(on_finished),
        };
        on_started();
        let hwnd = ::windows::Win32::Foundation::HWND(hwnd_raw as *mut std::ffi::c_void);
        for attempt in 1..=RETRY_ATTEMPTS {
            let mut slept = 0;
            while slept < RETRY_INTERVAL_MS && !is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(CANCEL_CHECK_MS));
                slept += CANCEL_CHECK_MS;
            }
            if is_cancelled() {
                log::info!("UIA selection retry cancelled before attempt {attempt}");
                guard.finish(None);
                return;
            }
            let result = (|| {
                let _com = ComGuard::init()?;
                let automation = uia_automation()?;
                let range = uia_selection_in_window(&automation, hwnd)?;
                return context_from_range(&range);
            })();
            match result {
                Ok(context) if !context.text.trim().is_empty() => {
                    // 查询本身可能耗时较久，补发前再判一次
                    if is_cancelled() {
                        log::info!("UIA selection retry cancelled after attempt {attempt}");
                        guard.finish(None);
                        return;
                    }
                    log::info!(
                        "UIA selection retry succeeded on attempt {attempt}: {}",
                        describe_context(&context)
                    );
                    guard.finish(Some(context));
                    return;
                }
                Ok(_) => {
                    log::info!("UIA selection retry attempt {attempt}: empty selection");
                }
                Err(error) => {
                    log::info!("UIA selection retry attempt {attempt}: {error}");
                }
            }
        }
        log::info!("UIA selection retry gave up after {RETRY_ATTEMPTS} attempts");
        guard.finish(None);
    });
}

/// BSTR（长度前缀的 UTF-16 宽串）转 String；含孤代理对时报错（不 panic、不截断）。
fn bstr_to_string(text: &BSTR) -> Result<String, String> {
    return String::try_from(text)
        .map_err(|error| format!("the text is not valid UTF-16: {error}"));
}

/// 模拟 Ctrl+C 回退：备份剪贴板 → 注入 Ctrl+C → 轮询剪贴板序号 → 读取新文本 →
/// 恢复备份。前台是终端时不注入（见 windows_target::foreground_terminal），按
/// “剪贴板未变”处理。
///
/// `silent_if_unchanged`：剪贴板序号在注入后未变化的语义。true（UIA 路径能
/// 正常响应、只是没读到选区）= 什么都没选，返回空串静默；false（UIA 查询
/// 本身失败）= 注入的输入没被任何文本控件接受，多为前台窗口以管理员身份运行
/// （UIPI 拦截），或取词目标压根不在前台（托盘菜单抢占前台且未归还，见
/// windows_target；此时调用方已先记一条诊断日志），返回 Err 由前端弹失败提示。
fn copy_via_ctrl_c(silent_if_unchanged: bool) -> Result<String, String> {
    if let Some(terminal) = super::windows_target::foreground_terminal() {
        log::info!("skipping the simulated Ctrl+C in a terminal ({terminal})");
        if silent_if_unchanged {
            return Ok(String::new());
        }
        return Err(format!(
            "the foreground window is a terminal ({terminal}); the simulated Ctrl+C was skipped \
             because it would interrupt the running process"
        ));
    }
    // 备份要在取序号之前：读取延迟渲染的格式会让来源应用写剪贴板，序号随之变化
    let backup = match clipboard_backup() {
        Ok(items) => {
            if items.is_empty() {
                log::info!("nothing on the clipboard to back up");
            }
            items
        }
        Err(error) => {
            log::warn!("failed to back up the clipboard: {error}");
            Vec::new()
        }
    };
    let sequence_before =
        unsafe { ::windows::Win32::System::DataExchange::GetClipboardSequenceNumber() };
    send_ctrl_c();
    // 轮询剪贴板序号而非固定 sleep：既快又能区分“复制成功”与“剪贴板未变”
    if !wait_clipboard_change(sequence_before) {
        // 序号没变 = 没复制到东西，剪贴板原封未动，不能再写（写回只会多一条历史记录）
        if silent_if_unchanged {
            return Ok(String::new());
        }
        return Err(
            "the clipboard did not change after the simulated Ctrl+C (the target window may not be \
             in the foreground, or it is running as administrator, which blocks injected input)"
                .to_string(),
        );
    }
    // 读取结果先存下：复制的可能是文件等非文本内容，读取失败时备份同样要恢复
    let text = clipboard_read_text();
    restore_clipboard_quietly(&backup);
    return text;
}

/// 恢复剪贴板备份（尽力而为，失败仅记日志）。备份为空时不动作：要么原本就没有内容，
/// 要么备份失败，此时清空剪贴板也换不回原内容。
///
/// 目标应用可能还占着剪贴板（慢应用写大段选区时），打开失败后多试几轮。
fn restore_clipboard_quietly(backup: &[ClipboardItem]) {
    const ATTEMPTS: usize = 3;
    const RETRY_INTERVAL_MS: u64 = 100;

    if backup.is_empty() {
        return;
    }
    for attempt in 1..=ATTEMPTS {
        match clipboard_write_items(backup) {
            Ok(()) => return,
            Err(error) => {
                log::warn!(
                    "failed to restore the clipboard after the simulated Ctrl+C \
                     (attempt {attempt}/{ATTEMPTS}): {error}"
                );
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(RETRY_INTERVAL_MS));
    }
}

/// 未分配的虚拟键，用来打断 Alt/Win 的“单按”判定（AutoHotkey 的 menu mask key 同款）。
const MENU_MASK_KEY: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

/// 一次 Ctrl+C 要注入的按键序列（虚拟键, 是否抬起）。`is_down` 查询修饰键当前是否按下。
///
/// 全局快捷键触发时用户往往还物理按着修饰键（如 Ctrl+Shift+S 的 Ctrl 和 Shift）；
/// 先注入各修饰键的抬起事件把组合“清空”，否则注入的 Ctrl+C 会与物理按住的
/// Shift 叠加成 Ctrl+Shift+C（浏览器里是打开开发者工具而非复制）。用户随后
/// 松开物理按键产生的多余抬起事件无害。
///
/// 只抬起确实按下的键，且 Alt/Win 抬起之前先按一下 `MENU_MASK_KEY`：RegisterHotKey
/// 吃掉了热键本身的按键，目标窗口只看到“Alt 按下、Alt 抬起”，会当成单按 Alt 进入菜单
/// 模式（随后的 Ctrl+C 就落空了），单按 Win 则弹出开始菜单。
fn ctrl_c_key_events(is_down: impl Fn(VIRTUAL_KEY) -> bool) -> Vec<(VIRTUAL_KEY, bool)> {
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_C, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };

    let mut events = Vec::new();
    if [VK_MENU, VK_LWIN, VK_RWIN].into_iter().any(&is_down) {
        events.push((MENU_MASK_KEY, false));
        events.push((MENU_MASK_KEY, true));
    }
    for modifier in [VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN, VK_CONTROL] {
        if is_down(modifier) {
            events.push((modifier, true));
        }
    }
    events.extend([
        (VK_CONTROL, false),
        (VK_C, false),
        (VK_C, true),
        (VK_CONTROL, true),
    ]);
    return events;
}

/// 通过 SendInput 注入一次 Ctrl+C（按键序列见 `ctrl_c_key_events`）。
fn send_ctrl_c() {
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP,
    };

    let key_input = |vk: VIRTUAL_KEY, key_up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // GetAsyncKeyState 的最高位（返回值为负）表示按键当前处于按下状态
    let inputs: Vec<INPUT> = ctrl_c_key_events(|vk| unsafe { GetAsyncKeyState(vk.0 as i32) } < 0)
        .into_iter()
        .map(|(vk, key_up)| key_input(vk, key_up))
        .collect();
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        log::warn!("SendInput injected {sent}/{} key events", inputs.len());
    }
}

/// 轮询剪贴板序号直到变化或超时（15ms 间隔、500ms 超时）；超时返回 false。
///
/// 超时要留足慢应用（大段选区）的余量：复制若在超时之后才落地，就没人恢复备份了，
/// 用户的剪贴板会被所选文本覆盖。这个代价只能压缩、无法归零（再晚到的复制无从感知）。
fn wait_clipboard_change(sequence_before: u32) -> bool {
    use ::windows::Win32::System::DataExchange::GetClipboardSequenceNumber;

    const POLL_INTERVAL_MS: u64 = 15;
    const TIMEOUT_MS: u128 = 500;

    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < TIMEOUT_MS {
        if unsafe { GetClipboardSequenceNumber() } != sequence_before {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
    }
    return false;
}

/// 打开剪贴板（带有限重试）：剪贴板是全局独占资源，其他程序（剪贴板管理器、
/// 虚拟机剪贴板共享代理等）可能短暂持有，瞬时打开失败重试几次即可恢复。
fn open_clipboard() -> Result<(), String> {
    use ::windows::Win32::System::DataExchange::OpenClipboard;

    const ATTEMPTS: usize = 10;
    const RETRY_INTERVAL_MS: u64 = 10;

    let mut last_error = String::new();
    for _ in 0..ATTEMPTS {
        match unsafe { OpenClipboard(None) } {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = error.to_string();
                std::thread::sleep(std::time::Duration::from_millis(RETRY_INTERVAL_MS));
            }
        }
    }
    return Err(format!("failed to open the clipboard: {last_error}"));
}

/// 读取剪贴板中的 Unicode 文本；剪贴板被占用或内容非文本时返回 Err。
fn clipboard_read_text() -> Result<String, String> {
    use ::windows::Win32::System::DataExchange::CloseClipboard;

    unsafe {
        open_clipboard()?;
        let result = read_clipboard_text_inner();
        let _ = CloseClipboard();
        return result;
    }
}

fn read_clipboard_text_inner() -> Result<String, String> {
    use ::windows::Win32::Foundation::HGLOBAL;
    use ::windows::Win32::System::DataExchange::GetClipboardData;
    use ::windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use ::windows::Win32::System::Ole::CF_UNICODETEXT;

    unsafe {
        let handle = GetClipboardData(CF_UNICODETEXT.0 as u32)
            .map_err(|error| format!("the clipboard has no Unicode text: {error}"))?;
        let memory = HGLOBAL(handle.0);
        let data = GlobalLock(memory);
        if data.is_null() {
            return Err("failed to lock the clipboard data".to_string());
        }
        // 剪贴板文本为 NUL 结尾的 UTF-16；以 GlobalSize 为上界扫描 NUL，防越界
        let capacity = GlobalSize(memory) / 2;
        let units = data as *const u16;
        let mut length = 0;
        while length < capacity && *units.add(length) != 0 {
            length += 1;
        }
        let text = String::from_utf16(std::slice::from_raw_parts(units, length))
            .map_err(|error| format!("the clipboard text is not valid UTF-16: {error}"));
        let _ = GlobalUnlock(memory);
        return text;
    }
}

/// 备份的一个剪贴板格式：格式 id 与其 HGLOBAL 内容的逐字节副本。
struct ClipboardItem {
    format: u32,
    bytes: Vec<u8>,
}

/// 备份总量上限。内容只在内存里停留一次取词的时间，上限只是防御超大剪贴板。
const BACKUP_MAX_BYTES: usize = 256 * 1024 * 1024;

/// 备份耗时预算。读取会触发来源应用的延迟渲染（Excel 大区域的图片格式可达数秒），
/// 而每次取词都要先备份，超预算后只保证纯文本，其余格式放弃。
const BACKUP_TIME_BUDGET: std::time::Duration = std::time::Duration::from_millis(200);

/// 恢复时附加的注册格式：不进 Win+V 历史、不同步到其他设备、第三方剪贴板监视器也跳过
/// （《Clipboard Formats》的 Cloud Clipboard and Clipboard History Formats 一节）。
/// 原内容当初已进过历史，恢复只是把它放回去，不该再多出一条。
const HISTORY_EXCLUSION_FORMATS: [&str; 3] = [
    "ExcludeClipboardContentFromMonitorProcessing",
    "CanIncludeInClipboardHistory",
    "CanUploadToCloudClipboard",
];

/// 注册格式中不备份的：OLE 内部数据与指向来源文档的对象/链接（来源应用不再持有剪贴板后
/// 已是失效引用，且 Embed Source 这类格式渲染代价极高），以及离开来源就没有内容的
/// 虚拟文件描述。这些格式缺失时 OLE 会退回读普通格式，粘贴不受影响。
const UNRESTORABLE_REGISTERED_FORMATS: [&str; 14] = [
    "Ole Private Data",
    "DataObject",
    "Embed Source",
    "Embedded Object",
    "Link Source",
    "Link Source Descriptor",
    "Custom Link Source",
    "Object Descriptor",
    "OwnerLink",
    "ObjectLink",
    "Native",
    "FileContents",
    "FileGroupDescriptor",
    "FileGroupDescriptorW",
];

/// 该格式能否按“HGLOBAL 逐字节复制”备份。`name` 是注册格式的名称（标准格式为 None）。
///
/// 白名单只收内容确为 HGLOBAL 的标准格式；CF_BITMAP/CF_PALETTE/CF_ENHMETAFILE 是
/// GDI 句柄，CF_METAFILEPICT 内含元文件句柄，CF_OWNERDISPLAY/CF_DSP* 依赖原所有者，
/// 私有区间的句柄类型无从确定——这些一律跳过。位图不受影响：系统总会从 CF_BITMAP 合成
/// CF_DIB。CF_TEXT/CF_OEMTEXT 由 CF_UNICODETEXT 合成，同样不必备份。只有元文件的
/// 剪贴板（极少见）无可备份内容，会被复制的文本覆盖。
fn is_restorable_format(format: u32, name: Option<&str>) -> bool {
    use ::windows::Win32::System::Ole::{
        CF_DIB, CF_DIBV5, CF_DIF, CF_HDROP, CF_LOCALE, CF_PENDATA, CF_RIFF, CF_SYLK, CF_TIFF,
        CF_UNICODETEXT, CF_WAVE,
    };

    /// 注册格式的 id 区间起点（RegisterClipboardFormat 返回 0xC000 到 0xFFFF）
    const REGISTERED_FORMAT_FIRST: u32 = 0xC000;

    if format >= REGISTERED_FORMAT_FIRST {
        return name.is_some_and(|name| {
            !UNRESTORABLE_REGISTERED_FORMATS.contains(&name)
                && !HISTORY_EXCLUSION_FORMATS.contains(&name)
        });
    }
    return [
        CF_SYLK,
        CF_DIF,
        CF_TIFF,
        CF_DIB,
        CF_PENDATA,
        CF_RIFF,
        CF_WAVE,
        CF_UNICODETEXT,
        CF_HDROP,
        CF_LOCALE,
        CF_DIBV5,
    ]
    .iter()
    .any(|standard| standard.0 as u32 == format);
}

/// 备份剪贴板的全部可恢复格式，保持枚举顺序（它是来源应用声明的格式优先级，
/// 粘贴方据此挑选，恢复时按同样顺序写回）。
///
/// 同时备份 RTF/HTML 等格式才能保住富文本：UIA 成功时也要经 Ctrl+C 调和，只备份纯文本
/// 的话，每次取词都会把用户复制的富内容抹成纯文本。
fn clipboard_backup() -> Result<Vec<ClipboardItem>, String> {
    use ::windows::Win32::System::DataExchange::CloseClipboard;

    unsafe {
        open_clipboard()?;
        let items = read_clipboard_items();
        let _ = CloseClipboard();
        return Ok(items);
    }
}

fn read_clipboard_items() -> Vec<ClipboardItem> {
    use ::windows::Win32::System::DataExchange::EnumClipboardFormats;
    use ::windows::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_UNICODETEXT};

    let dib = CF_DIB.0 as u32;
    let dibv5 = CF_DIBV5.0 as u32;
    let unicode_text = CF_UNICODETEXT.0 as u32;

    let started = std::time::Instant::now();
    let mut items: Vec<ClipboardItem> = Vec::new();
    let mut total_bytes = 0;
    let mut has_dib = false;
    let mut format = 0;
    loop {
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if !is_restorable_format(format, clipboard_format_name(format).as_deref()) {
            continue;
        }
        // CF_DIB 与 CF_DIBV5 互相合成，留一份即可（截图的像素数据很大）
        if has_dib && (format == dib || format == dibv5) {
            continue;
        }
        let is_text = format == unicode_text;
        if !is_text && started.elapsed() >= BACKUP_TIME_BUDGET {
            log::info!("clipboard backup is over its time budget, skipping format {format}");
            continue;
        }
        let max_size = if is_text {
            usize::MAX
        } else {
            BACKUP_MAX_BYTES.saturating_sub(total_bytes)
        };
        // 个别格式读不到很常见（来源应用拒绝渲染、数据不是 HGLOBAL），能存多少存多少
        let Some(bytes) = read_clipboard_bytes(format, max_size) else {
            continue;
        };
        total_bytes += bytes.len();
        has_dib |= format == dib || format == dibv5;
        items.push(ClipboardItem { format, bytes });
    }
    return items;
}

/// 注册格式的名称；标准格式没有名称，返回 None。
fn clipboard_format_name(format: u32) -> Option<String> {
    use ::windows::Win32::System::DataExchange::GetClipboardFormatNameW;

    let mut buffer = [0u16; 256];
    let length = unsafe { GetClipboardFormatNameW(format, &mut buffer) };
    if length <= 0 {
        return None;
    }
    return Some(String::from_utf16_lossy(&buffer[..length as usize]));
}

/// 逐字节复制一个格式的 HGLOBAL；句柄不是 HGLOBAL 时 GlobalSize 为 0，返回 None。
fn read_clipboard_bytes(format: u32, max_size: usize) -> Option<Vec<u8>> {
    use ::windows::Win32::Foundation::HGLOBAL;
    use ::windows::Win32::System::DataExchange::GetClipboardData;
    use ::windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    unsafe {
        let memory = HGLOBAL(GetClipboardData(format).ok()?.0);
        let size = GlobalSize(memory);
        if size == 0 || size > max_size {
            return None;
        }
        let data = GlobalLock(memory);
        if data.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(data as *const u8, size).to_vec();
        let _ = GlobalUnlock(memory);
        return Some(bytes);
    }
}

/// 用备份内容替换剪贴板（先清空），并附加历史排除标记。
fn clipboard_write_items(items: &[ClipboardItem]) -> Result<(), String> {
    use ::windows::Win32::System::DataExchange::CloseClipboard;

    unsafe {
        open_clipboard()?;
        let result = write_clipboard_items_inner(items);
        let _ = CloseClipboard();
        return result;
    }
}

fn write_clipboard_items_inner(items: &[ClipboardItem]) -> Result<(), String> {
    use ::windows::core::HSTRING;
    use ::windows::Win32::System::DataExchange::{EmptyClipboard, RegisterClipboardFormatW};

    unsafe {
        EmptyClipboard().map_err(|error| format!("failed to empty the clipboard: {error}"))?;
        // 单个格式写失败不中断：已清空的剪贴板能多放回一个格式就少丢一点
        for item in items {
            if let Err(error) = set_clipboard_bytes(item.format, &item.bytes) {
                log::warn!(
                    "failed to restore clipboard format {}: {error}",
                    item.format
                );
            }
        }
        // 三个格式的内容语义相容：两个 Can* 要求 DWORD 0，Exclude 接受任意数据
        let marker = 0u32.to_ne_bytes();
        for name in HISTORY_EXCLUSION_FORMATS {
            let format = RegisterClipboardFormatW(&HSTRING::from(name));
            if format == 0 {
                log::warn!("failed to register the clipboard format {name}");
                continue;
            }
            if let Err(error) = set_clipboard_bytes(format, &marker) {
                log::warn!("failed to set the clipboard format {name}: {error}");
            }
        }
        return Ok(());
    }
}

/// 把一段字节写入剪贴板的指定格式。SetClipboardData 成功后内存所有权移交系统，
/// 不得再 GlobalFree/GlobalUnlock；此前任何一步失败，内存都还归我们，必须释放。
fn set_clipboard_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
    use ::windows::Win32::Foundation::{GlobalFree, HANDLE};
    use ::windows::Win32::System::DataExchange::SetClipboardData;
    use ::windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
            .map_err(|error| format!("failed to allocate clipboard memory: {error}"))?;
        let data = GlobalLock(memory);
        if data.is_null() {
            let _ = GlobalFree(Some(memory));
            return Err("failed to lock the clipboard memory".to_string());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data as *mut u8, bytes.len());
        let _ = GlobalUnlock(memory);
        if let Err(error) = SetClipboardData(format, Some(HANDLE(memory.0))) {
            let _ = GlobalFree(Some(memory));
            return Err(format!("failed to set the clipboard data: {error}"));
        }
        return Ok(());
    }
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

    /// 冒烟测试：Ctrl+C 回退链路不应 panic（无桌面环境下允许返回 Err）。
    /// 注意：在有桌面环境运行时会真的注入按键并读写剪贴板，仅作开发期诊断。
    #[test]
    #[ignore = "injects keystrokes and touches the real clipboard"]
    fn ctrl_c_fallback_smoke_test() {
        let result = super::copy_via_ctrl_c(false);
        eprintln!(
            "copy_via_ctrl_c() -> {:?}",
            result.map(|text| text.chars().count())
        );
    }

    use super::{
        clipboard_backup, clipboard_read_text, clipboard_write_items, ctrl_c_key_events,
        is_restorable_format, ClipboardItem, RetryGuard, MENU_MASK_KEY, RETRYING,
    };
    use ::windows::Win32::System::Ole::{
        CF_BITMAP, CF_DIB, CF_ENHMETAFILE, CF_HDROP, CF_LOCALE, CF_METAFILEPICT, CF_OEMTEXT,
        CF_PALETTE, CF_PRIVATEFIRST, CF_TEXT, CF_UNICODETEXT,
    };
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_C, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const COPY_EVENTS: [(super::VIRTUAL_KEY, bool); 4] = [
        (VK_CONTROL, false),
        (VK_C, false),
        (VK_C, true),
        (VK_CONTROL, true),
    ];

    #[test]
    fn no_modifier_held_injects_a_bare_ctrl_c() {
        assert_eq!(ctrl_c_key_events(|_| false), COPY_EVENTS);
    }

    #[test]
    fn only_held_modifiers_are_released() {
        let events = ctrl_c_key_events(|vk| vk == VK_SHIFT || vk == VK_CONTROL);
        assert_eq!(events[..2], [(VK_SHIFT, true), (VK_CONTROL, true)]);
        assert_eq!(events[2..], COPY_EVENTS);
    }

    #[test]
    fn mask_key_precedes_the_release_of_alt_and_win() {
        for modifier in [VK_MENU, VK_LWIN] {
            let events = ctrl_c_key_events(|vk| vk == modifier);
            assert_eq!(
                events[..3],
                [
                    (MENU_MASK_KEY, false),
                    (MENU_MASK_KEY, true),
                    (modifier, true)
                ]
            );
            assert_eq!(events[3..], COPY_EVENTS);
        }
    }

    #[test]
    fn handle_based_standard_formats_are_not_restorable() {
        for format in [
            CF_BITMAP,
            CF_PALETTE,
            CF_METAFILEPICT,
            CF_ENHMETAFILE,
            CF_TEXT,
            CF_OEMTEXT,
            CF_PRIVATEFIRST,
        ] {
            assert!(!is_restorable_format(format.0 as u32, None), "{format:?}");
        }
    }

    #[test]
    fn memory_based_standard_formats_are_restorable() {
        for format in [CF_UNICODETEXT, CF_DIB, CF_HDROP, CF_LOCALE] {
            assert!(is_restorable_format(format.0 as u32, None), "{format:?}");
        }
    }

    #[test]
    fn registered_formats_are_filtered_by_name() {
        for name in ["HTML Format", "Rich Text Format", "PNG"] {
            assert!(is_restorable_format(0xC123, Some(name)), "{name}");
        }
        for name in [
            "Ole Private Data",
            "Embed Source",
            "CanIncludeInClipboardHistory",
        ] {
            assert!(!is_restorable_format(0xC123, Some(name)), "{name}");
        }
        assert!(!is_restorable_format(0xC123, None));
    }

    /// 备份恢复必须逐字节保真：富文本与含代理对/换行的文本都不能被改写或截断。
    /// GlobalSize 可能大于申请的长度，所以按前缀比较。
    #[test]
    #[ignore = "touches the real clipboard"]
    fn clipboard_backup_roundtrip_is_byte_exact() {
        use ::windows::core::HSTRING;
        use ::windows::Win32::System::DataExchange::RegisterClipboardFormatW;

        let text = "a😀b\r\n“词”  ";
        let text_bytes: Vec<u8> = text
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_ne_bytes)
            .collect();
        let html_format = unsafe { RegisterClipboardFormatW(&HSTRING::from("HTML Format")) };
        let html_bytes = b"Version:0.9\r\n<b>word</b>\0".to_vec();
        let items = [
            ClipboardItem {
                format: html_format,
                bytes: html_bytes.clone(),
            },
            ClipboardItem {
                format: CF_UNICODETEXT.0 as u32,
                bytes: text_bytes.clone(),
            },
        ];
        clipboard_write_items(&items).unwrap();
        assert_eq!(clipboard_read_text().unwrap(), text);
        let backup = clipboard_backup().unwrap();
        let find = |format: u32| backup.iter().find(|item| item.format == format).unwrap();
        assert!(find(html_format).bytes.starts_with(&html_bytes));
        assert!(find(CF_UNICODETEXT.0 as u32).bytes.starts_with(&text_bytes));
    }

    fn counting_guard(count: &Arc<AtomicUsize>) -> RetryGuard {
        let count = Arc::clone(count);
        return RetryGuard {
            on_finished: Some(Box::new(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
            })),
        };
    }

    /// 正常收尾、线程 panic 两条路径：on_finished 恰好一次、互斥标记都要复位。
    /// 两段写在同一个测试里，是因为 RETRYING 是全局状态，不能并行断言。
    #[test]
    fn retry_guard_finishes_exactly_once() {
        let count = Arc::new(AtomicUsize::new(0));

        RETRYING.store(true, Ordering::SeqCst);
        counting_guard(&count).finish(None);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(!RETRYING.load(Ordering::SeqCst));

        RETRYING.store(true, Ordering::SeqCst);
        let guard = counting_guard(&count);
        let panicked = std::thread::spawn(move || {
            let _guard = guard;
            panic!("simulated retry thread panic");
        })
        .join();
        assert!(panicked.is_err());
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(!RETRYING.load(Ordering::SeqCst));
    }
}
