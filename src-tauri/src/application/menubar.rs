//! macOS 专属：Dock 图标与菜单栏（托盘）图标的显隐管理。
//!
//! 图标显隐动态跟随主窗口可见性，分两种模式：
//! - 前台模式（主窗口可见，含启动时）：Dock 图标始终显示，菜单栏托盘始终不显示；
//! - 后台模式（主窗口被隐藏、应用保持运行，即 keep-running-on-close 拦截关闭）：
//!   按配置项 `background-icon` 决定——dock 显示 Dock 隐藏托盘、menu-bar 隐藏
//!   Dock 显示托盘、none 都隐藏。
//!
//! 主窗口恢复显示（点击 Dock 图标、划词快捷键、托盘菜单“打开”）时切回前台模式。
//! 其他平台均为空操作。

use tauri::{AppHandle, Manager};

use super::config::ConfigPath;
use super::logics;
use super::logics::config::{BackgroundIcon, Config};

/// 读取配置文件；读取失败时回退到与配置模板一致的默认值。
fn read_config_or_default(app: &AppHandle) -> Config {
    let config_path = app.state::<ConfigPath>();
    return logics::config::read_config(config_path.0.as_str()).unwrap_or_default();
}

/// 主窗口隐藏（点关闭按钮被拦截为隐藏）后调用：进入后台模式。
/// 按配置 background-icon 决定 Dock 图标与菜单栏托盘的显隐：
/// dock → 显示 Dock、隐藏托盘；menu-bar → 隐藏 Dock、显示托盘；none → 都隐藏。
pub fn on_main_window_hidden(app: &AppHandle) {
    let icon = read_config_or_default(app).background_icon();
    enter_background(app, icon);
}

/// 主窗口显示后调用：进入前台模式——Dock 图标始终显示、菜单栏托盘始终不显示。
pub fn on_main_window_shown(app: &AppHandle) {
    enter_foreground(app);
}

/// 配置变化（设置页保存 / 配置文件被外部编辑）后调用。
/// 主窗口处于隐藏（后台运行）时按新配置重新应用；前台模式的显隐与配置无关，不动作。
pub fn on_config_changed(app: &AppHandle) {
    let hidden = app
        .get_webview_window("main")
        .map(|w| !w.is_visible().unwrap_or(true))
        .unwrap_or(false);
    if hidden {
        on_main_window_hidden(app);
    }
}

/// 读取配置，返回“关闭窗口时保持后台运行”；读取失败时返回 true（保持默认行为兜底）。
///
/// 供主窗口关闭事件判断是拦截关闭（仅隐藏窗口）还是直接退出应用。
#[cfg(target_os = "macos")]
pub fn keep_running_on_close(app: &AppHandle) -> bool {
    let config = read_config_or_default(app);
    return config.keep_running_on_close();
}

/// 进入后台模式：按配置的图标位置应用 Dock 图标与菜单栏托盘的显隐（仅 macOS）。
#[cfg(target_os = "macos")]
fn enter_background(app: &AppHandle, icon: BackgroundIcon) {
    let dock_visible = matches!(icon, BackgroundIcon::Dock);
    let tray_visible = matches!(icon, BackgroundIcon::MenuBar);
    apply_dock_and_tray(app, dock_visible, tray_visible);
}

/// 其他平台：后台模式为空操作。
#[cfg(not(target_os = "macos"))]
fn enter_background(_app: &AppHandle, _icon: BackgroundIcon) {}

/// 进入前台模式：Dock 图标始终显示、菜单栏托盘始终不显示（仅 macOS）。
#[cfg(target_os = "macos")]
fn enter_foreground(app: &AppHandle) {
    apply_dock_and_tray(app, true, false);
}

/// 其他平台：前台模式为空操作。
#[cfg(not(target_os = "macos"))]
fn enter_foreground(_app: &AppHandle) {}

/// 应用 Dock 图标显隐并同步菜单栏托盘图标（仅 macOS）。
#[cfg(target_os = "macos")]
fn apply_dock_and_tray(app: &AppHandle, dock_visible: bool, tray_visible: bool) {
    // set_dock_visibility 内部经消息队列派发到主线程执行，任意线程调用均安全
    if let Err(error) = app.set_dock_visibility(dock_visible) {
        log::warn!("failed to set dock visibility to {dock_visible}: {error}");
    }
    update_tray(app, tray_visible);
}

/// 创建或更新菜单栏托盘图标，使其可见性与 `tray_visible` 一致。
///
/// 托盘一次性创建（仅在需要显示时），之后仅切换可见性，不销毁重建；
/// 托盘菜单提供“打开”与“退出”两个入口，保证隐藏 Dock 图标后仍可唤起/退出应用。
#[cfg(target_os = "macos")]
fn update_tray(app: &AppHandle, tray_visible: bool) {
    if let Some(tray) = app.tray_by_id("main") {
        // 托盘已创建：仅切换可见性
        if let Err(error) = tray.set_visible(tray_visible) {
            log::warn!("failed to set tray visibility to {tray_visible}: {error}");
        }
        return;
    }
    // 托盘不存在且无需显示：无事可做
    if !tray_visible {
        return;
    }
    // 托盘不存在且需要显示：创建托盘
    let open = match tauri::menu::MenuItem::with_id(
        app,
        "open",
        "打开 Anki 划词助手",
        true,
        None::<&str>,
    ) {
        Ok(item) => item,
        Err(error) => {
            log::warn!("failed to create tray menu item \"open\": {error}");
            return;
        }
    };
    let quit = match tauri::menu::MenuItem::with_id(
        app,
        "quit",
        "退出 Anki 划词助手",
        true,
        None::<&str>,
    ) {
        Ok(item) => item,
        Err(error) => {
            log::warn!("failed to create tray menu item \"quit\": {error}");
            return;
        }
    };
    let menu = match tauri::menu::Menu::with_items(app, &[&open, &quit]) {
        Ok(menu) => menu,
        Err(error) => {
            log::warn!("failed to create tray menu: {error}");
            return;
        }
    };
    let mut builder = tauri::tray::TrayIconBuilder::with_id("main")
        .tooltip("Anki 划词助手")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            // 打开主窗口：复用划词快捷键/Dock 点击的显示并聚焦逻辑
            "open" => {
                if let Err(error) = super::shortcut::show_and_focus_main_window(app) {
                    log::warn!("failed to show main window from tray: {error}");
                }
            }
            // 退出应用
            "quit" => app.exit(0),
            _ => {}
        });
    // 托盘图标使用应用默认窗口图标；缺失时跳过设置（托盘仍可用，仅无图标）
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    } else {
        log::warn!("no default window icon available for the tray icon");
    }
    if let Err(error) = builder.build(app) {
        log::warn!("failed to build tray icon: {error}");
    }
}
