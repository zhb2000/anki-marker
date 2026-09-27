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
//! 1. **记**：启动时起的后台跟踪线程，以固定间隔记下**最近一个“像应用窗口”的外来
//!    前台窗口**——即“用户最近所在的应用”。过滤掉本进程窗口、不可见窗口、工具窗口与
//!    不可激活窗口（托盘图标、浮出层、提示条都是这两类）、以及 shell/桌面窗口类。
//! 2. **还**：菜单触发的取词在捕获前 `SetForegroundWindow(记录窗口)` 并等待生效，
//!    之后 UIA 焦点元素与注入的 Ctrl+C 自然命中目标应用；后台重试锁定的 HWND 也随之
//!    正确（见 `retry_target`）。
//!
//! 记录带时间戳，且窗口持续为前台时**不断刷新**：记录语义是“用户当前所在的应用”，
//! 过期只发生在“前台长时间是本应用/不可用窗口”之后（此时旧目标已不可信，宁可不取词
//! 也不取错）。快捷键路径不经过托盘菜单，取词前会清掉记录（见 `forget_recorded_target`）。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use ::windows::Win32::Foundation::HWND;
use ::windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, SetForegroundWindow, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// 后台跟踪的轮询间隔。250ms 足够——用户切到某个应用再伸手去点托盘远不止这个时间；
/// 每轮只有两个轻量 API 调用。
const WATCH_INTERVAL: Duration = Duration::from_millis(250);

/// 记录的有效期：超过则不再用于归还。记录语义是“用户当前所在的应用”，窗口在前台期间
/// 时间戳不断刷新，所以这里衡量的是“用户离开目标应用多久了”。取 5 分钟：足够覆盖
/// “取词后在卡片里编辑一会儿再回托盘”，又不至于把一小时前的应用当成当前目标。
const RECORD_TTL: Duration = Duration::from_secs(300);

/// 归还前台的等待上限与轮询间隔。`SetForegroundWindow` 的生效是异步的（前台状态在该
/// 线程处理下一条消息时才结算），必须等它真正落到目标窗口再取词。
const RESTORE_TIMEOUT: Duration = Duration::from_millis(300);
const RESTORE_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 最近一个“用户所在的应用窗口”（HWND 数值 + 最近一次被观察到在前台的时刻）。
///
/// HWND 只是数值句柄、不含可析构资源，跨线程读写安全（同 selected_text::windows
/// 把 HWND 转 usize 跨线程的做法）。
static RECORDED_TARGET: Mutex<Option<(usize, Instant)>> = Mutex::new(None);

/// 启动前台窗口跟踪（应用 setup 阶段调用一次，Windows）。
///
/// 必须早于用户进入目标应用：记录的是“用户最近所在的应用”，若等托盘创建时才开始
/// 跟踪，用户此前在哪个应用里就已无从得知了。
pub fn install_foreground_watcher() {
    std::thread::spawn(|| loop {
        std::thread::sleep(WATCH_INTERVAL);
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() || !is_recordable(hwnd) {
            // 前台是本应用或不可用窗口（托盘菜单期间即如此）：保留上一次的记录
            continue;
        }
        let raw = hwnd.0 as usize;
        if recorded_raw() != Some(raw) {
            log::info!(
                "recorded the last foreign foreground window: {}",
                describe_window(hwnd)
            );
        }
        if let Ok(mut guard) = RECORDED_TARGET.lock() {
            // 每次采样都刷新时刻（记录衡量的是“多久没在目标应用里了”）
            *guard = Some((raw, Instant::now()));
        }
    });
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
            "the foreground window belongs to this process and no usable target window is \
             recorded (none seen recently); the capture will most likely miss the target app"
        );
        return;
    };
    log::info!(
        "the foreground window belongs to this process (taken by the tray menu); restoring {}",
        describe_window(target)
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
                describe_window(target)
            );
            return;
        }
        std::thread::sleep(RESTORE_POLL_INTERVAL);
    }
    log::warn!(
        "the foreground window did not switch to {} within {} ms; the capture will most likely \
         miss the target app",
        describe_window(target),
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

/// 取可用的记录目标（未过期、窗口仍存在且不属于本进程）；不可用时返回 None。
fn recorded_target() -> Option<HWND> {
    let (raw, recorded_at) = RECORDED_TARGET.lock().ok().and_then(|guard| *guard)?;
    if recorded_at.elapsed() > RECORD_TTL {
        return None;
    }
    let hwnd = HWND(raw as *mut std::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() || belongs_to_self(hwnd) {
        return None;
    }
    return Some(hwnd);
}

/// 已记录窗口的句柄数值（仅用于判断前台是否已切换，不做过期与有效性检查）。
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
fn describe_window(hwnd: HWND) -> String {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };
    return format!(
        "{:#x} (pid {pid}, class {:?})",
        hwnd.0 as usize,
        window_class(hwnd)
    );
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
