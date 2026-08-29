//! macOS 专属：Dock 图标与菜单栏（托盘）图标的显隐管理。
//!
//! 图标显隐动态跟随主窗口可见性，分两种模式：
//! - 前台模式（主窗口可见，含启动时）：Dock 图标始终显示，菜单栏托盘始终不显示；
//! - 后台模式（主窗口被隐藏、应用保持运行，即 keep-running-on-close 拦截关闭）：
//!   按配置项 `background-icon` 决定——dock 显示 Dock 隐藏托盘、menu-bar 隐藏
//!   Dock 显示托盘、none 都隐藏。
//!
//! 托盘菜单提供“划词录入”“打开”“退出”三个入口，Dock 图标菜单仅提供“划词录入”
//! （“打开/退出”由系统自带：左键点击 Dock 图标恢复窗口、右键菜单含系统“退出”）。
//! 这保证隐藏 Dock 图标后仍可划词录入/唤起/退出应用；“划词录入”与全局快捷键等效，
//! 为不设快捷键的用户提供鼠标操作入口，已设置快捷键时在菜单项中显示快捷键提示。
//! 两个菜单共用在 setup 阶段注册一次的 AppHandle::on_menu_event 事件处理。
//!
//! Dock 图标菜单：Tauri/tao 未提供 Dock 菜单 API，通过 ObjC runtime 在运行时向
//! tao 的 NSApplication 代理类注入 `applicationDockMenu:` 方法实现（见
//! install_dock_menu）；注入失败仅记录日志并优雅降级（无 Dock 菜单，其余功能不受影响）。
//!
//! 主窗口恢复显示（点击 Dock 图标、划词快捷键、托盘菜单）时切回前台模式。
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
/// 每次显示前重建托盘菜单，保证“划词录入”项的快捷键提示与最新配置一致。
#[cfg(target_os = "macos")]
fn update_tray(app: &AppHandle, tray_visible: bool) {
    if let Some(tray) = app.tray_by_id("main") {
        // 托盘已创建：仅切换可见性
        if let Err(error) = tray.set_visible(tray_visible) {
            log::warn!("failed to set tray visibility to {tray_visible}: {error}");
        }
        // 即将显示时重建菜单，刷新“划词录入”项的快捷键提示（配置可能已变化）
        if tray_visible {
            if let Some(menu) = build_tray_menu(app) {
                if let Err(error) = tray.set_menu(Some(menu)) {
                    log::warn!("failed to update tray menu: {error}");
                }
            }
        }
        return;
    }
    // 托盘不存在且无需显示：无事可做
    if !tray_visible {
        return;
    }
    // 托盘不存在且需要显示：创建托盘
    let menu = match build_tray_menu(app) {
        Some(menu) => menu,
        None => {
            log::warn!("failed to build tray menu, skip creating the tray icon");
            return;
        }
    };
    let mut builder = tauri::tray::TrayIconBuilder::with_id("main")
        .tooltip("Anki 划词助手")
        .menu(&menu)
        .show_menu_on_left_click(true);
    // 菜单事件由 register_menu_event_handler 在应用级统一处理（托盘与 Dock 菜单共用）
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

/// 构建托盘菜单：“划词录入”（与全局快捷键等效，已设置快捷键时在菜单项中显示提示）、
/// “打开”与“退出”；任一菜单项创建失败时返回 None（放弃本次创建/刷新，托盘保持原状）。
#[cfg(target_os = "macos")]
fn build_tray_menu(app: &AppHandle) -> Option<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};

    // 读取配置中的全局快捷键；读取失败视为未设置（与配置缺省值兜底策略一致）。
    // 快捷键字符串作为菜单项加速键提示显示（macOS 渲染为 ⌘⇧S 样式），
    // 无法解析时由 Tauri 静默忽略（仅不显示提示，不影响菜单项）。
    let shortcut = read_config_or_default(app).global_shortcut().to_string();
    let accelerator = if shortcut.is_empty() { None } else { Some(shortcut.as_str()) };
    let capture = match MenuItem::with_id(app, "capture", "划词录入", true, accelerator) {
        Ok(item) => item,
        Err(error) => {
            log::warn!("failed to create tray menu item \"capture\": {error}");
            return None;
        }
    };
    let open = match MenuItem::with_id(
        app,
        "open",
        "打开 Anki 划词助手",
        true,
        None::<&str>,
    ) {
        Ok(item) => item,
        Err(error) => {
            log::warn!("failed to create tray menu item \"open\": {error}");
            return None;
        }
    };
    let quit = match MenuItem::with_id(
        app,
        "quit",
        "退出 Anki 划词助手",
        true,
        None::<&str>,
    ) {
        Ok(item) => item,
        Err(error) => {
            log::warn!("failed to create tray menu item \"quit\": {error}");
            return None;
        }
    };
    let separator = match PredefinedMenuItem::separator(app) {
        Ok(item) => item,
        Err(error) => {
            log::warn!("failed to create tray menu separator: {error}");
            return None;
        }
    };
    return match Menu::with_items(app, &[&capture, &separator, &open, &quit]) {
        Ok(menu) => Some(menu),
        Err(error) => {
            log::warn!("failed to create tray menu: {error}");
            return None;
        }
    };
}

/// 注册托盘菜单与 Dock 图标菜单共用的菜单事件处理（三个入口：划词录入/打开/退出）。
///
/// 在应用 setup 阶段注册一次。AppHandle::on_menu_event 接收所有 muda 菜单事件
/// （Tauri 把 muda 事件汇入事件循环后广播给应用级监听器），按菜单项 id 分发；
/// 不能依赖托盘存在——后台图标为 dock 时托盘不会创建，但 Dock 菜单仍需可用。
#[cfg(target_os = "macos")]
pub fn register_menu_event_handler(app: &AppHandle) {
    app.on_menu_event(|app, event| match event.id.as_ref() {
        // 划词录入：与全局快捷键相同的捕获流程（读取选中文本后录入主窗口）。
        // 点击托盘/Dock 菜单不会激活本应用，选中文本仍来自用户当前所在的应用
        "capture" => super::shortcut::on_shortcut_pressed(app.clone()),
        // 打开主窗口：复用划词快捷键/Dock 点击的显示并聚焦逻辑
        "open" => {
            if let Err(error) = super::shortcut::show_and_focus_main_window(app) {
                log::warn!("failed to show main window from icon menu: {error}");
            }
        }
        // 退出应用
        "quit" => app.exit(0),
        _ => {}
    });
}

/// 安装 Dock 图标菜单（仅 macOS）：运行时向 tao 的 NSApplication 代理类注入
/// `applicationDockMenu:` 方法。AppKit 在用户右键（或长按）Dock 图标时调用该方法，
/// 以返回值作为 Dock 菜单内容；菜单每次按需重建，快捷键提示始终与最新配置一致。
///
/// Tauri/tao 未提供 Dock 菜单 API，故借助 ObjC runtime 注入（见模块文档）。
/// 调用时机：应用 setup 阶段（NSApplication 已创建、事件循环尚未启动，均可满足要求）。
#[cfg(target_os = "macos")]
pub fn install_dock_menu(app: &AppHandle) {
    // 存下 AppHandle 供 ObjC 回调（无法携带 Rust 上下文）取用
    if let Ok(mut guard) = DOCK_MENU_APP.lock() {
        *guard = Some(app.clone());
    }
    unsafe {
        let shared_app: *mut objc2::runtime::AnyObject =
            objc2::msg_send![objc2::class!(NSApplication), sharedApplication];
        if shared_app.is_null() {
            log::warn!("failed to get shared NSApplication, dock menu unavailable");
            return;
        }
        let delegate: *mut objc2::runtime::AnyObject =
            objc2::msg_send![shared_app, delegate];
        if delegate.is_null() {
            log::warn!("failed to get NSApplication delegate, dock menu unavailable");
            return;
        }
        // 代理类由 tao 运行时动态创建且未实现 applicationDockMenu:，
        // class_addMethod 对已注册类添加新方法是合法的；返回 NO 说明已存在同名方法
        // （tao 未来版本可能自行实现），此时放弃注入、优雅降级
        let delegate_class =
            objc2::ffi::object_getClass(delegate) as *mut objc2::runtime::AnyClass;
        let added = objc2::ffi::class_addMethod(
            delegate_class,
            objc2::sel!(applicationDockMenu:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(
                    *mut objc2::runtime::AnyObject,
                    objc2::runtime::Sel,
                    *mut objc2::runtime::AnyObject,
                ) -> *mut objc2::runtime::AnyObject,
                objc2::runtime::Imp,
            >(application_dock_menu),
            c"@@:@".as_ptr(),
        );
        if added.is_false() {
            log::warn!("applicationDockMenu: already implemented by the delegate, skip injecting the dock menu");
        }
    }
}

/// 供 ObjC 回调（applicationDockMenu:）取用的应用句柄。
/// AppHandle 为 Send + Sync 的引用计数句柄，可在静态变量中安全持有。
#[cfg(target_os = "macos")]
static DOCK_MENU_APP: std::sync::Mutex<Option<AppHandle>> = std::sync::Mutex::new(None);

thread_local! {
    /// 持有最近一次构建的 Dock 菜单（muda::Menu）。
    ///
    /// applicationDockMenu: 返回的 NSMenu 指针仅在 muda::Menu 存活期间有效
    /// （muda 文档明确要求），故将其保存在主线程的 thread_local 中保活，
    /// 直到下一次重建（下一次右键 Dock 图标）才释放；该回调只会由 AppKit
    /// 在主线程调用，thread_local 即为正确的存放位置。
    #[cfg(target_os = "macos")]
    static DOCK_MENU: std::cell::RefCell<Option<muda::Menu>> =
        const { std::cell::RefCell::new(None) };
}

/// NSApplicationDelegate.applicationDockMenu: 的注入实现：构建 Dock 菜单并返回其原生 NSMenu。
///
/// 方法签名对应类型编码 "@@:@"（返回 id，参数为 self、_cmd、application）。
#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn application_dock_menu(
    _this: *mut objc2::runtime::AnyObject,
    _cmd: objc2::runtime::Sel,
    _application: *mut objc2::runtime::AnyObject,
) -> *mut objc2::runtime::AnyObject {
    let app = match DOCK_MENU_APP.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(app) => app.clone(),
            None => return std::ptr::null_mut(),
        },
        Err(_) => return std::ptr::null_mut(),
    };
    let menu = match build_dock_menu(&app) {
        Some(menu) => menu,
        None => return std::ptr::null_mut(),
    };
    // 先取出 NSMenu 指针，再把 muda::Menu 存入 thread_local 保活（顺序不可颠倒）
    let ns_menu = muda::ContextMenu::ns_menu(&menu) as *mut objc2::runtime::AnyObject;
    DOCK_MENU.with(|slot| *slot.borrow_mut() = Some(menu));
    return ns_menu;
}

/// 构建 Dock 图标菜单：仅“划词录入”一项（与全局快捷键等效，已设置快捷键时在菜单项中
/// 显示提示）。菜单项 id 与托盘菜单一致，事件由同一处理器分发。
///
/// “打开”与“退出”无需自建：左键点击 Dock 图标即恢复窗口（Reopen 事件），
/// 且 macOS 会自动在 Dock 菜单中附带“退出”等系统项。
///
/// 直接使用 muda（Tauri 菜单的底层库）构建：Dock 菜单需要自行持有原生 NSMenu，
/// 而 Tauri 的菜单类型不暴露底层指针。回调在主线程执行，muda 对象可安全创建。
#[cfg(target_os = "macos")]
fn build_dock_menu(app: &AppHandle) -> Option<muda::Menu> {
    // 读取配置中的全局快捷键；读取失败视为未设置（与配置缺省值兜底策略一致）
    let shortcut = read_config_or_default(app).global_shortcut().to_string();
    let accelerator = if shortcut.is_empty() {
        None
    } else {
        match shortcut.parse::<muda::accelerator::Accelerator>() {
            Ok(accelerator) => Some(accelerator),
            // 与托盘菜单一致：解析失败仅不显示提示，不影响菜单项
            Err(error) => {
                log::warn!("failed to parse global shortcut \"{shortcut}\" for the dock menu: {error}");
                None
            }
        }
    };
    let capture = muda::MenuItem::with_id("capture", "划词录入", true, accelerator);
    return match muda::Menu::with_items(&[&capture]) {
        Ok(menu) => Some(menu),
        Err(error) => {
            log::warn!("failed to create dock menu: {error}");
            return None;
        }
    };
}
