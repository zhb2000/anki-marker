//! Windows 捕获目标窗口：托盘菜单抢占前台时，取词目标的识别与归还。
//!
//! ## 问题（2026-09-27/28 实机定位）
//!
//! 托盘菜单弹出前，tray-icon 会 `SetForegroundWindow` 自己的隐藏消息窗口（shell 要求，
//! 否则点菜单外部菜单不消失）。于是取词时前台与键盘焦点都在我们自己身上：UIA 的
//! `GetFocusedElement` 命中该隐藏窗口（它没有 TextPattern），SendInput 注入的 Ctrl+C
//! 也送进该窗口。实测菜单关闭后 800ms 前台仍停在该窗口（此时菜单已结算完：`capture`
//! /`menuOwner` 归零、`flags` 为 0），即 **Windows 不会自动把前台还给原应用**。
//!
//! 而且“菜单弹出前一瞬间的前台窗口”未必是用户要取词的应用：常见操作是
//! “快捷键取词 → 本应用窗口弹出并获得焦点 → 关掉窗口回后台 → 点托盘菜单”，
//! 隐藏窗口不会因此失去前台，所以点托盘的那一刻前台仍是我们自己的窗口。
//! （因此也不能靠子类化托盘窗口去读那一刻的前台：读到的往往是自己。）
//!
//! ## 做法
//!
//! 1. **记**：启动时起的跟踪线程安装 `EVENT_SYSTEM_FOREGROUND` 事件钩子
//!    （`WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS`），在前台窗口变化时记下
//!    **最近一个“像应用窗口”的外来前台窗口**——即“用户最近所在的应用”。过滤掉本进程
//!    窗口、不可见窗口、工具窗口与不可激活窗口（托盘图标、浮出层、提示条都是这些）、
//!    以及 shell/桌面窗口类。
//! 2. **还**：菜单触发的取词在捕获前 `SetForegroundWindow(记录窗口)` 并等待生效，
//!    之后 UIA 焦点元素与注入的 Ctrl+C 自然命中目标应用；后台重试锁定的 HWND 也随之
//!    正确（见 `retry_target`）。
//!
//! ## 为什么用钩子而不是轮询
//!
//! 轮询（曾用过 250ms 一轮）的代价是持续定时唤醒（约 240 次/分钟），而钩子在空闲时
//! 零唤醒——线程阻塞在 `GetMessage`，只在真正的前台变化时才被叫醒。文档对该机制给出
//! 的保证是明确的（MSAA《Out-of-Context Hook Functions》：“assures that the callback
//! function receives all events in the order in which they are generated”），因此不需要
//! 再做周期性对账轮询；代价是回调必须**极轻**（同一文档：“If a hook function does not
//! process events quickly enough, USER resources are lowered, eventually resulting in a
//! fault or extremely slow response times”），所以回调里只做过滤、写一个 HWND 与时刻，
//! 绝不做 UIA/COM/剪贴板这类事（同理，归还日志里的应用名只在归还路径解析，见
//! `describe_target_window`，不放进回调）。
//!
//! 两个必须知道的边界：
//! - 钩子只报**变化**，不报初始状态 → 安装前先取一次当前前台窗口播种（否则“静默启动、
//!   用户已经在某个应用里”时，在用户切走之前不会收到任何事件）。
//! - 作用域是**当前桌面**（`SetWinEventHook` 的 `idProcess = 0` 即
//!   “all processes on the current desktop”）→ 安全桌面（UAC/Ctrl+Alt+Del）上的事件
//!   收不到；但切到安全桌面并不改变我们桌面上的前台窗口，记录通常仍然有效。
//!
//! 记录**不设有效期**：它的语义就是“用户最近所在的那个应用窗口”，无论多久以前。
//! 这与快捷键路径同构——快捷键取的也是“当前应用里此刻的选区”，不管那选区是五分钟前
//! 还是三小时前划的。曾试过给记录设时间上限，但在事件驱动下没有意义：时刻只在检测到
//! 应用切换时才刷新，用户停在同一应用里读一小时也不会产生事件，于是“上限”会把完全
//! 有效的目标判成过期（典型症状是“取词后在卡片里编辑一会儿，再回托盘就取不到”）。
//! 记录的时刻只保留给日志用（`recorded_age`），便于事后发现“归还了一个很久以前的应用”。
//! 快捷键路径不经过托盘菜单，取词前会清掉记录（见 `forget_recorded_target`）。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use ::windows::core::PWSTR;
use ::windows::Win32::Foundation::{CloseHandle, HWND};
use ::windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use ::windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use ::windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowThreadProcessId,
    IsWindow, IsWindowVisible, SetForegroundWindow, EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE, MSG,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// 归还前台的等待上限与轮询间隔。`SetForegroundWindow` 的生效是异步的（前台状态在该
/// 线程处理下一条消息时才结算），必须等它真正落到目标窗口再取词。
const RESTORE_TIMEOUT: Duration = Duration::from_millis(300);
const RESTORE_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 最近一个“用户所在的应用窗口”（HWND 数值 + 记录时刻）。
///
/// HWND 只是数值句柄、不含可析构资源，跨线程读写安全（同 selected_text::windows
/// 把 HWND 转 usize 跨线程的做法）。
static RECORDED_TARGET: Mutex<Option<(usize, Instant)>> = Mutex::new(None);

/// 启动前台窗口跟踪（应用 setup 阶段调用一次，Windows）。
///
/// 必须早于用户进入目标应用：记录的是“用户最近所在的应用”，若等托盘创建时才开始
/// 跟踪，用户此前在哪个应用里就已无从得知了。安装钩子的线程同时负责泵消息——
/// out-of-context 钩子的事件只在安装它的线程处理消息时才被投递。
pub fn install_foreground_watcher() {
    std::thread::spawn(|| {
        // 播种：钩子只报变化，不报初始状态（静默启动、用户已在某应用里时尤其需要）
        record_foreground(unsafe { GetForegroundWindow() });

        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_event_proc),
                0, // 所有进程
                0, // 所有线程
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            log::warn!(
                "failed to install the foreground event hook; capturing from the tray menu will \
                 keep failing on Windows"
            );
            return;
        }
        // 消息泵：线程阻塞在 GetMessage 时零唤醒，事件在 GetMessage 内部被投递到
        // 上面的回调（本线程没有窗口，故无需 TranslateMessage/DispatchMessage）。
        // 循环只在收到 WM_QUIT（返回值 0）或出错（-1）时结束。
        let mut message = MSG::default();
        loop {
            if unsafe { GetMessageW(&mut message, None, 0, 0) }.0 <= 0 {
                break;
            }
        }
        let _ = unsafe { UnhookWinEvent(hook) };
    });
}

/// `EVENT_SYSTEM_FOREGROUND` 回调：前台窗口变化时记录候选目标。
///
/// 回调必须极轻（见模块文档），故只做过滤与一次记录写入，不做任何跨进程调用。
unsafe extern "system" fn foreground_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    if event != EVENT_SYSTEM_FOREGROUND {
        return;
    }
    record_foreground(hwnd);
}

/// 把候选窗口写入记录（并在目标变化时记一行日志）。
///
/// 候选不通过过滤时不动记录——保留“用户最近所在的那个应用”，这正是托盘菜单抢走前台
/// 之后我们唯一还能拿到的线索。
fn record_foreground(hwnd: HWND) {
    if hwnd.0.is_null() || !is_recordable(hwnd) {
        return;
    }
    let raw = hwnd.0 as usize;
    if recorded_raw() != Some(raw) {
        log::info!(
            "recorded the last foreign foreground window: {}",
            describe_window(hwnd)
        );
    }
    if let Ok(mut guard) = RECORDED_TARGET.lock() {
        *guard = Some((raw, Instant::now()));
    }
}

/// 是否值得记为取词目标——即“用户正在使用的应用窗口”。
///
/// 排除：本进程的窗口（主窗口/托盘窗口）、不可见窗口、工具窗口与不可激活窗口
/// （托盘图标、浮出层、提示条一律是 `WS_EX_TOOLWINDOW` 或 `WS_EX_NOACTIVATE`），
/// 以及 shell 与桌面的窗口类（点任务栏/通知区域时它们会短暂获得前台）。
fn is_recordable(hwnd: HWND) -> bool {
    if belongs_to_self(hwnd) || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return false;
    }
    let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    if ex_style & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 {
        return false;
    }
    return !is_shell_class(&window_class(hwnd));
}

/// shell 与桌面的窗口类：任务栏、通知区域及其浮出层宿主、桌面窗口。
/// 它们会因点击任务栏/通知区域而短暂成为前台窗口，不是取词目标。
fn is_shell_class(class: &str) -> bool {
    const SHELL_CLASSES: [&str; 8] = [
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
        "TrayNotifyWnd",
        "NotifyIconOverflowWindow",
        "Windows.UI.Core.CoreWindow",
        "XamlExplorerHostIslandWindow",
        "Progman",
        "WorkerW",
    ];
    return SHELL_CLASSES.contains(&class);
}

/// 菜单触发的取词在捕获前调用：若前台已被本应用的窗口占住（托盘菜单所致），把记下的
/// 用户窗口还回前台并等待生效。前台已是外来窗口（快捷键路径，或用户已切走）时不动作。
pub fn ensure_foreign_foreground() {
    let current = unsafe { GetForegroundWindow() };
    if !current.0.is_null() && !belongs_to_self(current) {
        return;
    }
    let Some(target) = recorded_target() else {
        log::warn!(
            "the foreground window belongs to this process and no target window is recorded \
             (none seen yet, or the recorded window is gone); the capture will most likely miss \
             the target app"
        );
        return;
    };
    // 目标窗口在整段归还流程里不变，应用名只解析一次（跨进程调用，别每行日志都查）
    let target_description = describe_target_window(target);
    log::info!(
        "the foreground window belongs to this process (taken by the tray menu); restoring {} \
         (recorded {} s ago)",
        target_description,
        recorded_age().map(|age| age.as_secs()).unwrap_or_default(),
    );
    let started = Instant::now();
    // 返回值只表示“调用是否被受理”，调用可能被前台锁拒绝，真正是否生效由下面的轮询
    // （GetForegroundWindow 是否已切换）判定
    let _ = unsafe { SetForegroundWindow(target) };
    // 轮询等待前台真正切换：SetForegroundWindow 的结果不是同步生效的
    while started.elapsed() < RESTORE_TIMEOUT {
        if unsafe { GetForegroundWindow() } == target {
            log::info!(
                "foreground window restored after {} ms: {}",
                started.elapsed().as_millis(),
                target_description
            );
            return;
        }
        std::thread::sleep(RESTORE_POLL_INTERVAL);
    }
    log::warn!(
        "the foreground window did not switch to {} within {} ms; the capture will most likely \
         miss the target app",
        target_description,
        RESTORE_TIMEOUT.as_millis()
    );
}

/// 后台重试应锁定的窗口：当前前台（若为外来窗口）优先，否则用记录的目标窗口。
///
/// 修复前这里用 `GetForegroundWindow()`，托盘路径下锁到的是我们自己的托盘窗口，
/// 重试只会空转；改用记录值后，即使归还失败，重试仍能在正确的窗口里找选区。
pub fn retry_target() -> Option<HWND> {
    let current = unsafe { GetForegroundWindow() };
    if !current.0.is_null() && !belongs_to_self(current) {
        return Some(current);
    }
    return recorded_target();
}

/// 当前前台窗口是否属于本进程（取词失败时用于诊断：多半是托盘菜单占着前台）。
pub fn foreground_is_own() -> bool {
    let hwnd = unsafe { GetForegroundWindow() };
    return !hwnd.0.is_null() && belongs_to_self(hwnd);
}

/// 丢弃已记录的目标窗口。
///
/// 供非菜单路径（全局快捷键）的取词调用：那条路径不经过托盘菜单，清掉记录可以避免
/// 陈旧记录被后续的重试目标解析捡到（重试可能把别的应用的选区当作本次结果补发）。
pub fn forget_recorded_target() {
    if let Ok(mut guard) = RECORDED_TARGET.lock() {
        *guard = None;
    }
}

/// 取可用的记录目标（窗口仍存在且不属于本进程）；无记录或记录已失效时返回 None。
///
/// 不设时间上限（见模块文档）：记录恒为“用户最近所在的应用窗口”。
fn recorded_target() -> Option<HWND> {
    let (raw, _) = RECORDED_TARGET.lock().ok().and_then(|guard| *guard)?;
    let hwnd = HWND(raw as *mut std::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() || belongs_to_self(hwnd) {
        return None;
    }
    return Some(hwnd);
}

/// 记录的年龄（仅用于日志：让“归还了一个很久以前的应用”在日志里可见）。
fn recorded_age() -> Option<Duration> {
    return RECORDED_TARGET
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .map(|(_, recorded_at)| recorded_at.elapsed());
}

/// 已记录窗口的句柄数值（仅用于判断目标是否变化，不做过期与有效性检查）。
fn recorded_raw() -> Option<usize> {
    return RECORDED_TARGET.lock().ok().and_then(|guard| *guard).map(|(raw, _)| raw);
}

/// 窗口是否属于本进程。
fn belongs_to_self(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };
    return pid == std::process::id();
}

/// 日志用的窗口描述：句柄 + 进程 id + 类名（类名足以辨认目标：Chrome_WidgetWin_1 /
/// Notepad / ApplicationFrameWindow 等）。
///
/// 只在钩子回调等“必须极轻”的路径使用；能负担跨进程调用的路径（归还）用
/// `describe_target_window`，那里会额外解析出应用名。
fn describe_window(hwnd: HWND) -> String {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };
    return format!(
        "{:#x} (pid {pid}, class {:?})",
        hwnd.0 as usize,
        window_class(hwnd)
    );
}

/// 归还路径的窗口描述：比 `describe_window` 多一个应用名（如 `chrome.exe`）。
///
/// Chromium 系应用的顶层窗口类都是 `Chrome_WidgetWin_1`（Chrome/Edge/Brave…），
/// 光看类名分不出是哪个应用，排查“归还给了谁”时应用名很关键。
fn describe_target_window(hwnd: HWND) -> String {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };
    let name = process_image_name(pid)
        .map(|name| format!("{name}, "))
        .unwrap_or_default();
    return format!(
        "{:#x} (pid {pid}, {name}class {:?})",
        hwnd.0 as usize,
        window_class(hwnd)
    );
}

/// 进程可执行文件名（如 `chrome.exe` / `msedge.exe`）；取不到时返回 None。
///
/// 这是**跨进程调用**（OpenProcess + 查询进程镜像路径），因此只在取词/归还线程上
/// 调用，绝不放钩子回调里（见模块文档），失败（权限不足、进程已退出、路径过长）时
/// 日志退化为不含应用名。
fn process_image_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // PROCESS_QUERY_LIMITED_INFORMATION：Vista 起对更高完整性级别（提权）的进程也能打开
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    // 路径可能超过 MAX_PATH，给足缓冲；仍不足时查询失败 → 退化不显示应用名
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    // 只取文件名；用 rsplit 兼容两种分隔符，不依赖 std::path 的宿主平台语义
    return Some(path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string());
}

/// 取窗口类名（GetClassNameW）；失败返回 "?"。
fn window_class(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if length <= 0 {
        return "?".to_string();
    }
    return String::from_utf16_lossy(&buffer[..length as usize]);
}
