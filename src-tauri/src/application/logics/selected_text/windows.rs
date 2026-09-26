//! Windows 取词实现：UI Automation（TextPattern）优先，失败回退模拟 Ctrl+C 读剪贴板。
//!
//! 与 macOS 版同构：UIA 无需特殊权限（读不了以管理员身份运行的前台窗口——
//! UIPI 会同时挡下 UIA 跨进程读取与 SendInput 注入，此时两级路径都会失败，
//! 由前端弹失败提示）。UIA 拿不到选区（控件不支持 TextPattern、GetSelection
//! 为空或“成功”但返回空串，如自绘文本的编辑器）时回退 SendInput 模拟 Ctrl+C，
//! 剪贴板备份/恢复是尽力而为的（非文本剪贴板跳过备份恢复，语义与 macOS 一致）。
//!
//! 选词取句（word-to-sentence）：UIA 不直接提供选区偏移，用同一套 TextRange
//! 机器推导——选区向两侧扩出上下文窗口（MoveEndpointByUnit 越界自动截停），
//! 再把窗口副本的终点收缩到选区起点，前缀文本的 UTF-16 长度即选区偏移
//! （窗口、前缀、选区来自同一提供方的同一接口，坐标系自洽），交给共享的
//! capture 模块校验锚点并切句。
//!
//! 后台重试：Chromium 系应用在 UIA 客户端查询时才按需物化无障碍树，首次查询
//! 可能撞上树未就绪——此时立即回退 Ctrl+C 保证录入不阻塞，同时后台轮询重跑
//! UIA 链路，树就绪后补发完整结果（语义同 macOS 的 spawn_selection_retry）。

use super::capture::{capture_in_context, SentenceCapture};
use super::SelectedContext;

use ::windows::core::BSTR;
use ::windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize};
use ::windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use ::windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextRange, TextUnit_Character,
    UIA_TextPatternId, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
};

pub fn get_selected_text() -> Result<String, String> {
    if let Ok(text) = uia_selected_text() {
        if !text.trim().is_empty() {
            return Ok(text);
        }
        // UIA “成功”但为空：无法区分“真没选”与“应用不暴露选区”，回退 Ctrl+C 再判一次
    }
    return get_selected_text_by_ctrl_c();
}

/// word_to_sentence 为 true 时尝试“选词取句”：UIA 全链路成功返回 {句子, 词}；
/// 拿到词但取句失败返回 {词原文, None}；UIA 报错或词为空时回退模拟 Ctrl+C。
/// word_to_sentence 为 false 时完全等同 get_selected_text 的行为。
///
/// `on_retry_captured`：UIA 失败回退后，若目标应用的无障碍树在后台物化
/// （Chromium 系按需激活），后台重试取到完整句子时经此回调补发结果。
pub fn get_selected_context(
    word_to_sentence: bool,
    on_retry_captured: impl FnOnce(SelectedContext) + Send + 'static,
) -> Result<SelectedContext, String> {
    if !word_to_sentence {
        return get_selected_text().map(|text| SelectedContext { text, word: None });
    }
    match uia_selected_context() {
        Ok(context) if !context.text.trim().is_empty() => return Ok(context),
        Ok(_) => {
            // UIA “成功”但为空：无法区分“真没选”与“应用不暴露选区”，回退 Ctrl+C
            // 再判一次；不触发后台重试，以免给“未选中就按快捷键”的常见操作增加延迟
            log::warn!(
                "UIA path returned empty selected text (no selection or the app does not \
                 expose its selection via UIA), falling back to simulated Ctrl+C"
            );
        }
        Err(error) => {
            // UIA 失败：立即回退 Ctrl+C 保证录入不阻塞；同时启动后台重试——
            // Chromium 系应用的无障碍树在被 UIA 客户端查询后按需物化，树就绪后补发句子。
            // 重试锁定当前前台窗口的 HWND 而不是跟随焦点：随后的回退结果/失败提示
            // 会弹出本应用窗口夺走焦点，跟随焦点会把查询打到我们自己的窗口上
            let hwnd = unsafe { ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
            if hwnd.0.is_null() {
                log::info!("no foreground window, skipping the background UIA selection retry");
            } else {
                log::info!("spawning background UIA selection retry");
                spawn_selection_retry(hwnd, on_retry_captured);
            }
            log::warn!("UIA path failed, falling back to simulated Ctrl+C: {error}");
        }
    }
    return get_selected_text_by_ctrl_c().map(|text| SelectedContext { text, word: None });
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
    let pattern: IUIAutomationTextPattern = unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) }
        .map_err(|error| {
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
    let candidates = unsafe { element.FindAll(TreeScope_Descendants, &condition) }
        .map_err(|error| format!("failed to search the window for TextPattern elements: {error}"))?;
    // 遍历上限：防御病态提供方返回巨量元素（正常窗口只有少数文本控件）
    let count = unsafe { candidates.Length() }.unwrap_or(0).min(64);
    for index in 0..count {
        let Ok(candidate) = (unsafe { candidates.GetElement(index) }) else { continue };
        let Ok(pattern) = (unsafe {
            candidate.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        }) else {
            continue;
        };
        let Ok(range) = text_pattern_selection(&pattern) else { continue };
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
                Anonymous: VARIANT_0_0_0 { boolVal: VARIANT_BOOL(-1) },
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
        return Ok(SelectedContext { text: word, word: None });
    }
    return Ok(match find_sentence_for_word(range, &word) {
        Ok(SentenceCapture::Expanded(sentence)) => SelectedContext {
            text: sentence,
            word: Some(word.trim().to_string()),
        },
        Ok(SentenceCapture::SelectionAsIs(text)) => SelectedContext { text, word: None },
        Err(error) => {
            // 已拿到词：取句失败不致命，降级为仅录入词原文
            log::warn!("failed to extract sentence for the selected word: {error}");
            SelectedContext { text: word, word: None }
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
        window.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -WINDOW_MARGIN)
    }
    .map_err(|error| format!("failed to extend the context window to the left: {error}"))?;
    unsafe { window.MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, WINDOW_MARGIN) }
        .map_err(|error| format!("failed to extend the context window to the right: {error}"))?;
    let context = bstr_to_string(
        &unsafe { window.GetText(-1) }
            .map_err(|error| format!("failed to read the context window: {error}"))?,
    )?;

    // 选区在窗口内的 UTF-16 偏移：前缀范围（窗口副本终点收缩到选区起点）的文本长度
    let prefix = unsafe { window.Clone() }
        .map_err(|error| format!("failed to clone the context window: {error}"))?;
    unsafe {
        prefix.MoveEndpointByRange(TextPatternRangeEndpoint_End, selection, TextPatternRangeEndpoint_Start)
    }
    .map_err(|error| format!("failed to shrink the prefix range to the selection start: {error}"))?;
    // 硬校验：前缀终点必须恰好落在选区起点（MoveEndpointByRange 静默失灵的防线）。
    // 不过则整个取句链路作废、降级录词——宁可不取句，也不切错句子
    match unsafe {
        prefix.CompareEndpoints(TextPatternRangeEndpoint_End, selection, TextPatternRangeEndpoint_Start)
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

/// 后台重试：Chromium 系应用在 UIA 客户端查询时才按需物化无障碍树，首次查询
/// 可能失败。按固定间隔轮询目标窗口（锁定 HWND 重跑取句链路，不跟随系统焦点——
/// 失败/成功弹窗会夺走焦点），取到非空选区后经 `on_captured` 补发完整结果；
/// 超时放弃。
fn spawn_selection_retry<F>(hwnd: ::windows::Win32::Foundation::HWND, on_captured: F)
where
    F: FnOnce(SelectedContext) + Send + 'static,
{
    use std::sync::atomic::Ordering;

    /// 轮询次数与间隔（总计约 6 秒，覆盖 Chromium 首次物化无障碍树的耗时）
    const RETRY_ATTEMPTS: usize = 10;
    const RETRY_INTERVAL_MS: u64 = 600;

    if RETRYING.swap(true, Ordering::SeqCst) {
        return;
    }
    // HWND 是窗口句柄（实为数值，不含可析构资源），可安全跨线程传递
    let hwnd_raw = hwnd.0 as usize;
    std::thread::spawn(move || {
        let hwnd = ::windows::Win32::Foundation::HWND(hwnd_raw as *mut std::ffi::c_void);
        for attempt in 1..=RETRY_ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(RETRY_INTERVAL_MS));
            let result = (|| {
                let _com = ComGuard::init()?;
                let automation = uia_automation()?;
                let range = uia_selection_in_window(&automation, hwnd)?;
                return context_from_range(&range);
            })();
            match result {
                Ok(context) if !context.text.trim().is_empty() => {
                    log::info!("UIA selection retry succeeded on attempt {attempt}");
                    RETRYING.store(false, Ordering::SeqCst);
                    on_captured(context);
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
        RETRYING.store(false, Ordering::SeqCst);
    });
}

/// BSTR（长度前缀的 UTF-16 宽串）转 String；含孤代理对时报错（不 panic、不截断）。
fn bstr_to_string(text: &BSTR) -> Result<String, String> {
    return String::try_from(text)
        .map_err(|error| format!("the text is not valid UTF-16: {error}"));
}

/// 模拟 Ctrl+C 回退：备份文本剪贴板 → 注入 Ctrl+C → 轮询剪贴板序号 →
/// 读取新文本 → 恢复备份。剪贴板备份/恢复是尽力而为的：当前内容不是文本
/// （图片/文件）时跳过备份与恢复、只做复制读取（代价与 macOS 回退一致：
/// 此类场景下原剪贴板内容会被复制的文本覆盖）。
fn get_selected_text_by_ctrl_c() -> Result<String, String> {
    let backup = clipboard_read_text().ok();
    if backup.is_none() {
        log::info!("no text on the clipboard to back up (or the clipboard is busy)");
    }
    let sequence_before = unsafe { ::windows::Win32::System::DataExchange::GetClipboardSequenceNumber() };
    send_ctrl_c();
    // 轮询剪贴板序号而非固定 sleep：既快又能区分“复制成功”与“前台应用不理会
    // 注入的输入”（超时多为 UIPI——前台窗口以管理员身份运行，SendInput 被静默丢弃）
    wait_clipboard_change(sequence_before)?;
    let text = clipboard_read_text()?;
    if let Some(backup) = backup {
        if let Err(error) = clipboard_write_text(&backup) {
            log::warn!("failed to restore the clipboard after the simulated Ctrl+C: {error}");
        }
    }
    return Ok(text);
}

/// 通过 SendInput 注入一次 Ctrl+C。
///
/// 全局快捷键触发时用户往往还物理按着修饰键（如 Ctrl+Shift+S 的 Ctrl 和 Shift）；
/// 先注入各修饰键的抬起事件把组合“清空”，否则注入的 Ctrl+C 会与物理按住的
/// Shift 叠加成 Ctrl+Shift+C（浏览器里是打开开发者工具而非复制）。用户随后
/// 松开物理按键产生的多余抬起事件无害。
fn send_ctrl_c() {
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VK_C, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VIRTUAL_KEY,
    };

    let key_input = |vk: VIRTUAL_KEY, key_up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [
        key_input(VK_SHIFT, true),
        key_input(VK_MENU, true),
        key_input(VK_LWIN, true),
        key_input(VK_RWIN, true),
        key_input(VK_CONTROL, true),
        key_input(VK_CONTROL, false),
        key_input(VK_C, false),
        key_input(VK_C, true),
        key_input(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        log::warn!("SendInput injected {sent}/{} key events", inputs.len());
    }
}

/// 轮询剪贴板序号直到变化或超时（15ms 间隔、300ms 超时）。
fn wait_clipboard_change(sequence_before: u32) -> Result<(), String> {
    use ::windows::Win32::System::DataExchange::GetClipboardSequenceNumber;

    const POLL_INTERVAL_MS: u64 = 15;
    const TIMEOUT_MS: u128 = 300;

    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < TIMEOUT_MS {
        if unsafe { GetClipboardSequenceNumber() } != sequence_before {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
    }
    return Err(
        "the clipboard did not change after the simulated Ctrl+C (the foreground window may be \
         running as administrator, which blocks injected input)"
            .to_string(),
    );
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

/// 把文本写入剪贴板（先清空；SetClipboardData 成功后内存所有权移交系统，
/// 不得再 GlobalFree/GlobalUnlock）。
fn clipboard_write_text(text: &str) -> Result<(), String> {
    use ::windows::Win32::System::DataExchange::CloseClipboard;

    unsafe {
        open_clipboard()?;
        let result = write_clipboard_text_inner(text);
        let _ = CloseClipboard();
        return result;
    }
}

fn write_clipboard_text_inner(text: &str) -> Result<(), String> {
    use ::windows::Win32::Foundation::HANDLE;
    use ::windows::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
    use ::windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use ::windows::Win32::System::Ole::CF_UNICODETEXT;

    unsafe {
        EmptyClipboard().map_err(|error| format!("failed to empty the clipboard: {error}"))?;
        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let memory = GlobalAlloc(GMEM_MOVEABLE, utf16.len() * 2)
            .map_err(|error| format!("failed to allocate clipboard memory: {error}"))?;
        let data = GlobalLock(memory);
        if data.is_null() {
            return Err("failed to lock the clipboard memory".to_string());
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), data as *mut u16, utf16.len());
        let _ = GlobalUnlock(memory);
        SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(memory.0)))
            .map_err(|error| format!("failed to set the clipboard data: {error}"))?;
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    /// 冒烟测试：UIA 调用路径不应 panic；无桌面环境（CI）下允许返回 Err。
    #[test]
    fn uia_smoke_test() {
        let result = super::uia_selected_text();
        eprintln!("uia_selected_text() -> {:?}", result.map(|text| text.chars().count()));
    }

    /// 冒烟测试：Ctrl+C 回退链路不应 panic（无桌面环境下允许返回 Err）。
    /// 注意：在有桌面环境运行时会真的注入按键并读写剪贴板，仅作开发期诊断。
    #[test]
    #[ignore = "injects keystrokes and touches the real clipboard"]
    fn ctrl_c_fallback_smoke_test() {
        let result = super::get_selected_text_by_ctrl_c();
        eprintln!("get_selected_text_by_ctrl_c() -> {:?}", result.map(|text| text.chars().count()));
    }
}
