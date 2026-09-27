//! Windows 捕获目标窗口：托盘菜单抢占前台时的目标识别与归还。
//!
//! ## 问题（2026-09-27 实机定位）
//!
//! 托盘菜单弹出前，tray-icon 会对自己的隐藏消息窗口调 `SetForegroundWindow`
//! （类名 `tray_icon_app`；shell 要求这么做，否则点菜单外部菜单不会消失）。于是取词时
//! 前台与键盘焦点都落在我们自己身上：UIA 的 `GetFocusedElement` 命中该隐藏窗口
//! （它没有 TextPattern），SendInput 注入的 Ctrl+C 也送进该窗口。实测菜单关闭后
//! 800ms 前台仍停在该窗口（此时菜单已结算完：`capture`/`menuOwner` 归零、`flags` 为 0），
//! 即 **Windows 不会自动把前台还给原应用**，必须显式归还。
//!
//! ## 做法
//!
//! 1. **记**：子类化托盘消息窗口，在 `WM_RBUTTONDOWN`/`WM_RBUTTONUP`/`WM_CONTEXTMENU`
//!    中记下当时的前台窗口。子类化过程先于窗口自身的窗口过程，因此这些消息早于
//!    tray-icon 的 `SetForegroundWindow`，此刻前台仍是用户所在的应用。
//!    （不能用 `on_tray_icon_event`：实测该事件被派发时菜单已经弹出、前台已被抢走。）
//! 2. **还**：菜单触发的取词在捕获前 `SetForegroundWindow(记下的窗口)` 并等待生效，
//!    之后 UIA 焦点元素与注入的 Ctrl+C 自然命中目标应用；后台重试锁定的 HWND 也随之
//!    正确（见 `retry_target`）。
//!
//! 记录带时间戳，只在“当前前台确实属于本进程”且记录尚新鲜时使用，避免陈旧记录影响
//! 快捷键路径。找不到托盘窗口（如 tray-icon 将来改了类名）时只记警告并降级：
//! 不记录，菜单触发的取词退回修复前的行为（报取词失败）。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use ::windows::core::BOOL;
use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use ::windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use ::windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, IsWindow,
    SetForegroundWindow, WM_CONTEXTMENU, WM_RBUTTONDOWN, WM_RBUTTONUP,
};

/// tray-icon 创建托盘消息窗口时注册的类名（其 `TrayIcon::new` 中的字面量）。
/// Tauri 未暴露该 HWND，只能按类名在本进程内枚举（类名在桌面范围可重名，必须限定 pid）。
const TRAY_WINDOW_CLASS: &str = "tray_icon_app";

/// 子类化标识：同一窗口可挂多个子类化过程，用 id 区分
const SUBCLASS_ID: usize = 1;

/// 记录的有效期：超过则不再用于归还。菜单路径的正常时序是“右键 → 点菜单项”（几秒），
/// 给足余量即可——无限保鲜会让快捷键路径吃到陈旧的目标窗口。
const RECORD_TTL: Duration = Duration::from_secs(60);

/// 归还前台的等待上限与轮询间隔。`SetForegroundWindow` 的生效是异步的（前台状态在
/// 该线程处理下一条消息时才结算），必须等它真正落到目标窗口再取词。
const RESTORE_TIMEOUT: Duration = Duration::from_millis(300);
const RESTORE_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 菜单弹出前的前台窗口（HWND 数值 + 记录时刻）。
///
/// HWND 只是数值句柄、不含可析构资源，跨线程读写安全（同 selected_text::windows
/// 把 HWND 转 usize 跨线程的做法）。
static RECORDED_TARGET: Mutex<Option<(usize, Instant)>> = Mutex::new(None);

/// 安装托盘窗口的前台记录器（在托盘图标创建成功后调用，此时托盘窗口才存在）。
pub fn install_tray_recorder() {
    let Some(hwnd) = find_tray_window() else {
        log::warn!(
            "tray message window not found, the pre-menu foreground window cannot be recorded; \
             capturing from the tray menu will keep failing on Windows"
        );
        return;
    };
    if unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) }.as_bool() {
        log::info!("tray foreground recorder installed on the tray message window");
    } else {
        log::warn!("failed to subclass the tray message window, the pre-menu foreground window cannot be recorded");
    }
}

/// 在本进程内按类名找到托盘消息窗口（隐藏的顶级窗口，EnumWindows 能枚举到）。
fn find_tray_window() -> Option<HWND> {
    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let found = unsafe { &mut *(lparam.0 as *mut Option<HWND>) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };
        if pid == std::process::id() && window_class(hwnd) == TRAY_WINDOW_CLASS {
            *found = Some(hwnd);
            return BOOL(0); // 找到即停止枚举
        }
        return BOOL(1);
    }

    let mut found: Option<HWND> = None;
    let _ = unsafe { EnumWindows(Some(callback), LPARAM(&mut found as *mut _ as isize)) };
    return found;
}

/// 子类化过程：右键呼出菜单之前记下前台窗口，再原样交给 tray-icon 自己的窗口过程
/// （只做旁听，绝不截流）。
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid_subclass: usize,
    _ref_data: usize,
) -> LRESULT {
    if matches!(msg, WM_RBUTTONDOWN | WM_RBUTTONUP | WM_CONTEXTMENU) {
        record_foreground_before_menu();
    }
    return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
}

/// 记下当前前台窗口（菜单弹出前的用户所在窗口）。
fn record_foreground_before_menu() {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() || belongs_to_self(hwnd) {
        // 已经是我们自己的窗口：保留上一次的记录，不要用它覆盖
        return;
    }
    log::info!(
        "recorded the pre-menu foreground window: {}",
        describe_window(hwnd)
    );
    if let Ok(mut guard) = RECORDED_TARGET.lock() {
        *guard = Some((hwnd.0 as usize, Instant::now()));
    }
}

/// 菜单触发的取词在捕获前调用：若前台已被本应用的窗口占住（托盘菜单所致），把记下的
/// 用户窗口还回前台并等待生效。前台已是外来窗口（快捷键路径，或系统/用户已归还）时
/// 不做任何事。
pub fn ensure_foreign_foreground() {
    let current = unsafe { GetForegroundWindow() };
    if !current.0.is_null() && !belongs_to_self(current) {
        return;
    }
    let Some(target) = recorded_target() else {
        log::warn!(
            "the foreground window belongs to this process and no fresh pre-menu target window \
             was recorded; the capture will most likely miss the target app"
        );
        return;
    };
    log::info!(
        "the foreground window belongs to this process (taken by the tray menu); restoring {}",
        describe_window(target)
    );
    let started = Instant::now();
    // 返回值只表示“调用是否被受理”，调用可能被前台锁拒绝，真正是否生效由下面的
    // 轮询（GetForegroundWindow 是否已切换）判定
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

/// 后台重试应锁定的窗口：当前前台（若为外来窗口）优先，否则用新鲜记录的目标窗口。
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
/// 供非菜单路径（全局快捷键）的取词调用：那条路径不经过托盘菜单，清掉上一条记录可以
/// 避免陈旧记录被后续的重试目标解析捡到（重试可能把别的应用的选区当作本次结果补发）。
pub fn forget_recorded_target() {
    if let Ok(mut guard) = RECORDED_TARGET.lock() {
        *guard = None;
    }
}

/// 取新鲜的（未过期、窗口仍存在且不属于本进程的）记录目标；不可用时返回 None。
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
