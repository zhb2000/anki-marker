//! Windows 侧托盘图标所需的平台探测：任务栏深浅、图标位图尺寸、深浅变更监听。
//!
//! ## 为什么不能用 Tauri 的 theme()
//!
//! 托盘图标跟随系统深浅时，要跟随的是**任务栏**的深浅，对应注册表
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\SystemUsesLightTheme`
//! （设置页「个性化 › 颜色 › 选择模式 › 自定义 › 默认 Windows 模式」）。它与应用深浅
//! `AppsUseLightTheme`（默认应用模式）是**两个独立值**，而 Tauri/tao 的主题检测读的
//! 是后者（tao `platform_impl/windows/dark_mode.rs` 的 `read_apps_use_light_theme`），
//! 因此在「Windows 模式深 + 应用模式浅」这一常见组合下会判错，必须自己读注册表。
//!
//! ## 为什么用 WM_SETTINGCHANGE 而非 WinRT 的 UISettings 事件
//!
//! `UISettings.ColorValuesChanged` 在官方文档「WinRT APIs not supported in desktop apps」
//! 的 Unsupported members → Events 表中明确列为桌面应用不支持（实测 Windows 10 不触发、
//! Windows 11 触发，行为随版本变化）；且它的取值只反映**应用**深浅（实测其 Background
//! 恒随 AppsUseLightTheme 变化），与任务栏无关。`WM_SETTINGCHANGE` 是纯 Win32 广播，
//! 切换任一模式都会发出（lParam 为 "ImmersiveColorSet"），且到达时注册表已是新值。
//!
//! ## 为什么挂在主窗口上
//!
//! `WM_SETTINGCHANGE` 经 `HWND_BROADCAST` 广播，只发给**顶级窗口**，message-only
//! window 收不到（MSDN：message-only window "does not receive broadcast messages"）。
//! 主窗口在后台模式下只是隐藏而未被销毁，仍是顶级窗口，故直接对其做子类化。
//! 将来若支持「无窗口的纯托盘常驻」，需改为自建一个隐藏的顶级窗口。

use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use windows::core::w;
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON, WM_SETTINGCHANGE};

/// 个性化设置（主题深浅、强调色、透明效果等）所在注册表路径
const PERSONALIZE: windows::core::PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");

/// 子类化过程的标识：同一窗口可挂多个子类化过程，用 id 区分
const SUBCLASS_ID: usize = 1;

/// 变更回调：系统深浅变化时调用，用于刷新托盘图标
type ChangeHandler = fn(&AppHandle);

/// 监听器状态。`Mutex` 仅保护这两个字段，回调本身在锁外执行（回调会调用
/// Shell_NotifyIcon，避免持锁重入窗口过程）。
static LISTENER: Mutex<Option<ListenerState>> = Mutex::new(None);

struct ListenerState {
    /// 托盘图标刷新所需的 AppHandle
    app: AppHandle,
    /// 系统设置变更时的回调
    on_change: ChangeHandler,
}

/// 任务栏是否为深色。
///
/// 注册表键缺失时按**深色**处理：该键随 Light 主题在 Windows 10 1903（19H1）引入，
/// 更早的系统（Win10 早期、Win7、Win8.1）没有它，而那时的任务栏恒为深色。本应用需要
/// WebView2（Windows 10 2004 起自带），2004 > 1903，故该分支近乎不可达，仅作防御。
pub fn taskbar_is_dark() -> bool {
    return !system_uses_light_theme().unwrap_or(false);
}

/// 读 `SystemUsesLightTheme`：非 0 为浅色；键缺失或读取失败返回 None
fn system_uses_light_theme() -> Option<bool> {
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PERSONALIZE,
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    return (status == ERROR_SUCCESS).then_some(data != 0);
}

/// 托盘图标的位图尺寸：取系统小图标尺寸（`SM_CXSMICON`，随主显示器 DPI 缩放变化，
/// 100% = 16、150% = 20~24、200% = 32），归到最接近的一档出图资产。
/// 直接交给系统缩放单一尺寸会发虚，16px 那档尤其明显。
pub fn tray_icon_pixel_size() -> u32 {
    let cx = unsafe { GetSystemMetrics(SM_CXSMICON) };
    return match cx {
        ..=16 => 16,
        17..=20 => 20,
        21..=24 => 24,
        _ => 32,
    };
}

/// 对主窗口挂子类化过程监听系统设置变更，每次变更调用 `on_change`。
///
/// 挂载失败（如 comctl32 v6 未加载）时仅记录警告并降级：托盘图标只在创建时按当时
/// 状态选一次图，不再随系统切换实时变化。
pub fn install_taskbar_theme_listener(app: &AppHandle, on_change: ChangeHandler) {
    let Some(window) = app.get_webview_window("main") else {
        log::warn!("main window not found, skip installing the taskbar theme listener");
        return;
    };
    let hwnd = match window.hwnd() {
        Ok(hwnd) => hwnd,
        Err(error) => {
            log::warn!(
                "failed to get the main window handle, skip installing the taskbar theme listener: {error}"
            );
            return;
        }
    };
    if let Ok(mut guard) = LISTENER.lock() {
        *guard = Some(ListenerState {
            app: app.clone(),
            on_change,
        });
    } else {
        log::warn!("taskbar theme listener state is poisoned, skip installing");
        return;
    }
    let installed = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) };
    if !installed.as_bool() {
        log::warn!(
            "failed to subclass the main window, the tray icon will not follow taskbar theme changes"
        );
    }
}

/// 子类化过程：收到 `WM_SETTINGCHANGE` 即回调，由调用方重读状态并自行判断是否需要换图。
///
/// 不在此处过滤：切换「Windows 模式」与「应用模式」都广播 "ImmersiveColorSet"（还可能出现
/// "WindowsThemeElement" 等值），而显示缩放变化同样走这条广播（会影响托盘槽位尺寸）。
/// 统一交由调用方“重读并比对”处理，既不会漏，又天然去抖。
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid_subclass: usize,
    _ref_data: usize,
) -> LRESULT {
    if msg == WM_SETTINGCHANGE {
        if let Some((app, on_change)) = take_listener() {
            on_change(&app);
        }
    }
    // 必须继续传递：tao 自身也在处理 WM_SETTINGCHANGE（窗口标题栏/边框深浅等），
    // 子类化只做旁听，绝不能截流
    return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
}

/// 取回调所需的 `(AppHandle, 回调)`；未安装时返回 None。
/// 锁内只做取值，回调在锁外执行。
fn take_listener() -> Option<(AppHandle, ChangeHandler)> {
    let guard = LISTENER.lock().ok()?;
    let state = guard.as_ref()?;
    return Some((state.app.clone(), state.on_change));
}
