//! 临时诊断探针（Windows）：采样前台窗口与其键盘焦点归属。
//!
//! ## 为什么需要它
//!
//! Windows 上从托盘菜单点“划词录入”必然报“获取选中文本失败”，而全局快捷键正常。
//! 结构性原因是托盘菜单弹出前，tray-icon 会先 `SetForegroundWindow` 自己的隐藏消息
//! 窗口（shell 要求，否则点菜单外部菜单不消失，见 tray-icon `show_tray_menu`），
//! 于是 UIA 的 GetFocusedElement 与 SendInput 注入的 Ctrl+C 都打在我们自己身上。
//!
//! 但“该怎么修”取决于几个尚未实测的事实：
//!
//! 1. 菜单关闭后，前台窗口会不会**自动**回到目标应用？回到哪个窗口、多久？
//!    （若会 → 捕获前等一等即可；若不会 → 必须记下目标窗口再显式 SetForegroundWindow 还原）
//! 2. 期间**键盘焦点**（GetGUIThreadInfo 的 hwndFocus）到底在谁身上？托盘隐藏窗口是
//!    `WS_EX_NOACTIVATE`，它不能被激活——若焦点其实从未离开目标控件，那前台归属才是
//!    关键，修复方向也随之不同。
//! 3. 托盘点击事件（`on_tray_icon_event`）到达时，前台是不是还是目标应用？即能否在
//!    菜单弹出前就把目标窗口记下来。
//!
//! ## 使用与移除
//!
//! 只在 Windows 编译；采样只读，不改变任何窗口状态。时间线采样会占用捕获线程约
//! 800ms（随后才真正取词），因此它同时兼作“延迟后再捕获”的探针：若延迟后取词成功，
//! 说明等前台归还是可行方向。
//!
//! 日志级别为 info：**debug 构建**（stdout + 日志文件）可见；release 构建只记 Warn 以上，
//! 看不到这些采样（见 main.rs 的日志配置）。
//!
//! 证据收集完成后，本模块应删除或收敛为修复的一部分。

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
};

/// 时间线采样时刻（相对捕获线程启动，毫秒）：覆盖“菜单刚关闭 → 完全结算”的窗口期，
/// 末点 800ms 也是随后真正取词的时刻。
const SAMPLE_OFFSETS_MS: [u64; 7] = [0, 50, 100, 200, 300, 500, 800];

/// 菜单触发路径的采样：在捕获前按固定时刻记录前台窗口状态（只读，不改变状态）。
///
/// 调用约定：在捕获线程内调用（函数会阻塞，直到走完最后一个采样点）。
pub fn log_menu_trigger_timeline() {
    let start = Instant::now();
    for offset in SAMPLE_OFFSETS_MS {
        let target = Duration::from_millis(offset);
        if let Some(remaining) = target.checked_sub(start.elapsed()) {
            std::thread::sleep(remaining);
        }
        log_foreground_snapshot(&format!("menu +{offset}ms"));
    }
}

/// 记录一次前台窗口快照：前台 HWND、其进程/线程 id、窗口类名，以及该线程的 GUI 状态
/// （hwndActive/hwndFocus/hwndCapture/hwndMenuOwner）。`(our process)` 标记用于一眼分辨
/// 前台是否已经落到本应用（托盘隐藏窗口、菜单窗口、主窗口都算）。
pub fn log_foreground_snapshot(tag: &str) {
    let mut line = String::new();
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        line.push_str("no foreground window");
    } else {
        let mut pid = 0u32;
        let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };
        let _ = write!(
            line,
            "hwnd {}, pid {pid}{}, thread {thread_id}, class {:?}",
            hwnd_hex(hwnd),
            if pid == std::process::id() { " (our process)" } else { "" },
            window_class(hwnd),
        );
        match gui_thread_state(thread_id) {
            Some(state) => {
                let _ = write!(line, ", gui: {state}");
            }
            None => line.push_str(", gui: unavailable"),
        }
    }
    log::info!("foreground[{tag}]: {line}");
}

/// 窗口句柄的十六进制表示（HWND 的 Debug 输出带指针类型噪音，这里统一口径便于比对）。
fn hwnd_hex(hwnd: HWND) -> String {
    return format!("{:#x}", hwnd.0 as usize);
}

/// 取窗口类名（GetClassNameW）；失败返回 "?"。
///
/// 关键类名：`tray_icon_app` = tray-icon 的隐藏消息窗口（菜单属主）、`#32768` = 菜单窗口、
/// `Shell_TrayWnd`/`TrayNotifyWnd` = 任务栏与通知区域、`Progman`/`WorkerW` = 桌面。
fn window_class(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if length <= 0 {
        return "?".to_string();
    }
    return String::from_utf16_lossy(&buffer[..length as usize]);
}

/// 取指定线程的 GUI 状态摘要；线程 id 无效（前台窗口属于已退出线程等）时返回 None。
fn gui_thread_state(thread_id: u32) -> Option<String> {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread_id, &mut info) }.ok()?;
    return Some(format!(
        "active {}, focus {}, capture {}, menuOwner {}, flags {:#x}",
        hwnd_hex(info.hwndActive),
        hwnd_hex(info.hwndFocus),
        hwnd_hex(info.hwndCapture),
        hwnd_hex(info.hwndMenuOwner),
        info.flags.0,
    ));
}
