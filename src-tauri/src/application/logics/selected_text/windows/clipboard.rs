//! 模拟 Ctrl+C 回退：剪贴板的打开与读写、全格式备份与恢复、注入 Ctrl+C 的按键序列，
//! 以及等待剪贴板序号变化。

use super::super::windows_target;

use ::windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;

/// 模拟 Ctrl+C 回退：备份剪贴板 → 注入 Ctrl+C → 轮询剪贴板序号 → 读取新文本 →
/// 恢复备份。前台是终端时不注入（见 windows_target::foreground_terminal），按
/// “剪贴板未变”处理。
///
/// `silent_if_unchanged`：剪贴板序号在注入后未变化的语义。true（UIA 路径能
/// 正常响应、只是没读到选区）= 什么都没选，返回空串静默；false（UIA 查询
/// 本身失败）= 注入的输入没被任何文本控件接受，多为前台窗口以管理员身份运行
/// （UIPI 拦截），或取词目标压根不在前台（托盘菜单抢占前台且未归还，见
/// windows_target；此时调用方已先记一条诊断日志），返回 Err 由前端弹失败提示。
pub(super) fn copy_via_ctrl_c(silent_if_unchanged: bool) -> Result<String, String> {
    if let Some(terminal) = windows_target::foreground_terminal() {
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
        is_restorable_format, ClipboardItem, MENU_MASK_KEY,
    };
    use ::windows::Win32::System::Ole::{
        CF_BITMAP, CF_DIB, CF_ENHMETAFILE, CF_HDROP, CF_LOCALE, CF_METAFILEPICT, CF_OEMTEXT,
        CF_PALETTE, CF_PRIVATEFIRST, CF_TEXT, CF_UNICODETEXT,
    };
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_C, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT,
    };

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
}
