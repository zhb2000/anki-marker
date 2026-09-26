//! 托盘图标（三端）与 Dock 图标（仅 macOS）的显隐管理。
//!
//! 图标显隐动态跟随主窗口可见性，分两种模式：
//! - 前台模式（主窗口可见，含启动时）：macOS 显示 Dock 图标、隐藏托盘；
//!   Windows/Linux 隐藏托盘（任务栏即应用的“图标”，托盘仅在后台运行期间出现）；
//! - 后台模式（主窗口被隐藏、应用保持运行，即 keep-running-on-close 拦截关闭，
//!   或登录自启动静默拉起）：按配置项 `background-icon` 决定——macOS 依次为
//!   dock（显示 Dock 隐藏托盘）、menu-bar（隐藏 Dock 显示托盘）、none（都隐藏）；
//!   Windows/Linux 无 Dock 概念，仅有托盘，menu-bar（配置值 dock 亦按其处理）
//!   显示托盘、none 隐藏。
//!
//! 托盘菜单提供“打开”“退出”两个入口；macOS 额外提供“划词录入”（与全局快捷键
//! 等效，为不设快捷键的用户提供鼠标操作入口，已设置快捷键时在菜单项中显示快捷键
//! 提示；全局快捷键目前仅 macOS 支持）。这保证应用进入后台后仍可划词录入/唤起/
//! 退出应用。macOS 的 Dock 图标菜单仅提供“划词录入”（“打开/退出”由系统自带：
//! 左键点击 Dock 图标恢复窗口、右键菜单含系统“退出”）。两个菜单共用在 setup
//! 阶段注册一次的 AppHandle::on_menu_event 事件处理。
//!
//! 托盘点击行为按平台惯例区分：Windows 左键单击直接打开主窗口、菜单仅右键弹出
//! （点击事件经 AppHandle::on_tray_icon_event 处理）；macOS 左键即弹菜单；Linux
//! 依赖的 appindicator 托盘不支持点击事件，任何点击均由系统弹出菜单。
//!
//! 托盘图标的样式由配置项 `tray-icon-style` 决定：Windows 为「单色跟随系统/彩色/白/黑」，
//! Linux 为「彩色/白/黑」，macOS 不参与（其菜单栏图标为 template 图标，由系统按菜单栏
//! 明暗与高亮自动着色）。Windows 的「跟随系统」要判断的是**任务栏**深浅，见
//! super::windows_tray——它与 Tauri 的 App::theme()（读应用深浅）并非同一个值。
//!
//! Dock 图标菜单：Tauri/tao 未提供 Dock 菜单 API，通过 ObjC runtime 在运行时向
//! tao 的 NSApplication 代理类注入 `applicationDockMenu:` 方法实现（见
//! install_dock_menu）；注入失败仅记录日志并优雅降级（无 Dock 菜单，其余功能不受影响）。
//!
//! 主窗口恢复显示（点击 Dock 图标、划词快捷键、托盘菜单/图标）时切回前台模式。

use tauri::{AppHandle, Manager};

use super::config::ConfigPath;
use super::logics;
use super::logics::config::{BackgroundIcon, Config};
#[cfg(not(target_os = "macos"))]
use super::logics::config::TrayIconStyle;
#[cfg(not(target_os = "macos"))]
use std::sync::Mutex;

/// 读取配置文件；读取失败时回退到与配置模板一致的默认值。
fn read_config_or_default(app: &AppHandle) -> Config {
    let config_path = app.state::<ConfigPath>();
    return logics::config::read_config(config_path.0.as_str()).unwrap_or_default();
}

/// 主窗口隐藏（点关闭按钮被拦截为隐藏，或静默启动）后调用：进入后台模式。
/// 按配置 background-icon 决定应用图标的显隐：
/// macOS 上 dock → 显示 Dock、隐藏托盘，menu-bar → 隐藏 Dock、显示托盘，none → 都隐藏；
/// Windows/Linux 上仅控制托盘：menu-bar（含 dock）→ 显示托盘，none → 隐藏托盘。
pub fn on_main_window_hidden(app: &AppHandle) {
    let icon = read_config_or_default(app).background_icon();
    enter_background(app, icon);
}

/// 主窗口显示后调用：进入前台模式——macOS 显示 Dock 图标，三端均隐藏托盘。
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
pub fn keep_running_on_close(app: &AppHandle) -> bool {
    let config = read_config_or_default(app);
    return config.keep_running_on_close();
}

/// 进入后台模式：按配置的图标位置应用 Dock 图标与菜单栏托盘的显隐。
///
/// macOS：dock → 显示 Dock 隐藏托盘、menu-bar → 隐藏 Dock 显示托盘、none → 都隐藏。
/// Windows/Linux：无 Dock 概念，仅控制托盘——menu-bar（含 dock 值）显示托盘、none 隐藏。
fn enter_background(app: &AppHandle, icon: BackgroundIcon) {
    #[cfg(target_os = "macos")]
    {
        let dock_visible = matches!(icon, BackgroundIcon::Dock);
        let tray_visible = matches!(icon, BackgroundIcon::MenuBar);
        apply_dock_and_tray(app, dock_visible, tray_visible);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let tray_visible = !matches!(icon, BackgroundIcon::None);
        update_tray(app, tray_visible);
    }
}

/// 进入前台模式：macOS 显示 Dock 图标，三端均隐藏托盘。
fn enter_foreground(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    apply_dock_and_tray(app, true, false);
    #[cfg(not(target_os = "macos"))]
    update_tray(app, false);
}

/// 应用 Dock 图标显隐并同步菜单栏托盘图标（仅 macOS）。
#[cfg(target_os = "macos")]
fn apply_dock_and_tray(app: &AppHandle, dock_visible: bool, tray_visible: bool) {
    // set_dock_visibility 内部经消息队列派发到主线程执行，任意线程调用均安全
    if let Err(error) = app.set_dock_visibility(dock_visible) {
        log::warn!("failed to set dock visibility to {dock_visible}: {error}");
    }
    update_tray(app, tray_visible);
}

/// 托盘图标的渲染图（仅 macOS）：单色模板图标（纯黑+alpha，与 Windows/Linux 同源，
/// 由 build-tray-icons.mjs 从 icons/tray/tray-icon-mono.svg 按 36px 导出），
/// 设为模板后由系统自动适配菜单栏明暗模式与高亮反色，无需为深色模式单独出图。
#[cfg(target_os = "macos")]
fn tray_icon_image() -> tauri::image::Image<'static> {
    tauri::include_image!("icons/tray/tray-icon.png")
}

/// 托盘图标的实际形态：配置的 TrayIconStyle 按平台能力落地后的结果
#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayIconShape {
    /// 彩色应用图标（浅色/深色底均可辨识）
    Color,
    /// 白色单色图标（深色底）
    MonoWhite,
    /// 黑色单色图标（浅色底）
    MonoBlack,
}

/// 解析应当使用的托盘图标形态（Windows）。
/// auto：按任务栏深浅决定——浅色任务栏配黑图、深色任务栏配白图。
#[cfg(target_os = "windows")]
fn resolve_tray_icon_shape(app: &AppHandle) -> TrayIconShape {
    return match read_config_or_default(app).tray_icon_style() {
        TrayIconStyle::Auto => match super::windows_tray::taskbar_is_dark() {
            true => TrayIconShape::MonoWhite,
            false => TrayIconShape::MonoBlack,
        },
        TrayIconStyle::Color => TrayIconShape::Color,
        TrayIconStyle::White => TrayIconShape::MonoWhite,
        TrayIconStyle::Black => TrayIconShape::MonoBlack,
    };
}

/// 解析应当使用的托盘图标形态（Linux）。
/// auto：Linux 无可靠的托盘底色检测手段（面板配色随主题/发行版而异，单色图在相反底色
/// 上会直接不可见），故等同 Color；深色面板需用户显式选择白色。
#[cfg(target_os = "linux")]
fn resolve_tray_icon_shape(app: &AppHandle) -> TrayIconShape {
    return match read_config_or_default(app).tray_icon_style() {
        TrayIconStyle::Auto | TrayIconStyle::Color => TrayIconShape::Color,
        TrayIconStyle::White => TrayIconShape::MonoWhite,
        TrayIconStyle::Black => TrayIconShape::MonoBlack,
    };
}

/// 托盘图标位图尺寸：Windows 按系统小图标尺寸（随 DPI）选档；Linux 的图标由宿主面板
/// 自行缩放到面板尺寸，固定用 32 档。
#[cfg(target_os = "windows")]
fn tray_icon_pixel_size() -> u32 {
    return super::windows_tray::tray_icon_pixel_size();
}

#[cfg(target_os = "linux")]
fn tray_icon_pixel_size() -> u32 {
    return 32;
}

/// 按形态与位图尺寸取托盘图标（Windows/Linux）：单色稿按 DPI 出 16/20/24/32 四档，
/// 彩色仅有 32 一档（由系统缩放）。Linux 恒以 32 档调用（尺寸由宿主面板缩放）。
#[cfg(not(target_os = "macos"))]
fn tray_icon_image(shape: TrayIconShape, size: u32) -> tauri::image::Image<'static> {
    return match (shape, size) {
        (TrayIconShape::Color, _) => tauri::include_image!("icons/32x32.png"),
        (TrayIconShape::MonoWhite, 16) => tauri::include_image!("icons/tray/mono-white-16.png"),
        (TrayIconShape::MonoWhite, 20) => tauri::include_image!("icons/tray/mono-white-20.png"),
        (TrayIconShape::MonoWhite, 24) => tauri::include_image!("icons/tray/mono-white-24.png"),
        (TrayIconShape::MonoWhite, _) => tauri::include_image!("icons/tray/mono-white-32.png"),
        (TrayIconShape::MonoBlack, 16) => tauri::include_image!("icons/tray/mono-black-16.png"),
        (TrayIconShape::MonoBlack, 20) => tauri::include_image!("icons/tray/mono-black-20.png"),
        (TrayIconShape::MonoBlack, 24) => tauri::include_image!("icons/tray/mono-black-24.png"),
        (TrayIconShape::MonoBlack, _) => tauri::include_image!("icons/tray/mono-black-32.png"),
    };
}

/// 已应用的托盘图标（形态 + 位图尺寸）。
///
/// 未变化时不重复调用 set_icon：一次主题切换会广播多条 WM_SETTINGCHANGE，靠比对
/// 天然去抖；也避免系统深浅实际未变时做无谓的 Shell_NotifyIcon 调用。
#[cfg(not(target_os = "macos"))]
static APPLIED_TRAY_ICON: Mutex<Option<(TrayIconShape, u32)>> = Mutex::new(None);

/// 重新应用托盘图标：配置变更或系统设置（深浅/缩放）变化后调用。
/// 形态与尺寸均未变化（或托盘尚未创建）时不动作。
#[cfg(not(target_os = "macos"))]
pub fn refresh_tray_icon(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else {
        // 托盘尚未创建：创建时会按当时状态选图，无需处理
        return;
    };
    let shape = resolve_tray_icon_shape(app);
    let size = tray_icon_pixel_size();
    if last_applied_tray_icon() == Some((shape, size)) {
        return;
    }
    // 先记账再换图：万一 set_icon 期间窗口过程被重入，可避免重复换图陷入递归
    set_last_applied_tray_icon((shape, size));
    if let Err(error) = tray.set_icon(Some(tray_icon_image(shape, size))) {
        // 失败则清除记录，让后续刷新仍会重试
        clear_last_applied_tray_icon();
        log::warn!("failed to update the tray icon: {error}");
    }
}

#[cfg(not(target_os = "macos"))]
fn last_applied_tray_icon() -> Option<(TrayIconShape, u32)> {
    return match APPLIED_TRAY_ICON.lock() {
        Ok(guard) => *guard,
        Err(_) => None,
    };
}

/// 记录已应用的图标形态（托盘创建时由 update_tray 调用）
#[cfg(not(target_os = "macos"))]
fn set_last_applied_tray_icon(key: (TrayIconShape, u32)) {
    if let Ok(mut guard) = APPLIED_TRAY_ICON.lock() {
        *guard = Some(key);
    }
}

#[cfg(not(target_os = "macos"))]
fn clear_last_applied_tray_icon() {
    if let Ok(mut guard) = APPLIED_TRAY_ICON.lock() {
        *guard = None;
    }
}

/// 安装系统设置变更监听（仅 Windows）：任务栏深浅或显示缩放变化时刷新托盘图标。
/// 需在窗口创建之后调用（见 main.rs setup）。
#[cfg(target_os = "windows")]
pub fn install_tray_theme_listener(app: &AppHandle) {
    super::windows_tray::install_taskbar_theme_listener(app, refresh_tray_icon);
}

/// 左键单击托盘图标是否弹出菜单：
/// macOS 与 Linux（appindicator）左键弹菜单；Windows 左键留给“打开主窗口”，
/// 菜单仅右键弹出（点击事件见 register_tray_icon_event_handler）。
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn show_menu_on_left_click() -> bool {
    true
}

#[cfg(target_os = "windows")]
fn show_menu_on_left_click() -> bool {
    false
}

/// 创建或更新托盘图标，使其可见性与 `tray_visible` 一致。
///
/// 托盘一次性创建（仅在需要显示时），之后仅切换可见性，不销毁重建；
/// 每次显示前重建托盘菜单，保证菜单内容（如 macOS 的快捷键提示）与最新配置一致。
fn update_tray(app: &AppHandle, tray_visible: bool) {
    if let Some(tray) = app.tray_by_id("main") {
        // 托盘已创建：仅切换可见性
        if let Err(error) = tray.set_visible(tray_visible) {
            log::warn!("failed to set tray visibility to {tray_visible}: {error}");
        }
        // 即将显示时重建菜单，刷新菜单内容（配置可能已变化）
        if tray_visible {
            if let Some(menu) = build_tray_menu(app) {
                if let Err(error) = tray.set_menu(Some(menu)) {
                    log::warn!("failed to update tray menu: {error}");
                }
            }
        }
        // 图标样式也可能已变化（配置热更新会走到这里）
        #[cfg(not(target_os = "macos"))]
        refresh_tray_icon(app);
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
    // 图标形态与位图尺寸：Windows 的「跟随系统」需读任务栏深浅，并按 DPI 选尺寸档
    #[cfg(not(target_os = "macos"))]
    let (icon_shape, icon_size) = (resolve_tray_icon_shape(app), tray_icon_pixel_size());
    // 菜单事件由 register_menu_event_handler 在应用级统一处理（托盘与 Dock 菜单共用）
    let builder = tauri::tray::TrayIconBuilder::with_id("main")
        .tooltip("Anki 划词助手")
        .menu(&menu)
        .show_menu_on_left_click(show_menu_on_left_click());
    // macOS：单色模板图标（纯黑+alpha），设为模板后由系统自动适配菜单栏明暗与高亮反色
    #[cfg(target_os = "macos")]
    let builder = builder.icon(tray_icon_image()).icon_as_template(true);
    // Windows/Linux：按配置样式选图（彩色/白单色/黑单色）
    #[cfg(not(target_os = "macos"))]
    let builder = builder.icon(tray_icon_image(icon_shape, icon_size));
    match builder.build(app) {
        // 记录已应用的图标，供后续主题/配置变化时比对是否需要换图
        #[cfg(not(target_os = "macos"))]
        Ok(_) => set_last_applied_tray_icon((icon_shape, icon_size)),
        #[cfg(target_os = "macos")]
        Ok(_) => {}
        Err(error) => log::warn!("failed to build tray icon: {error}"),
    }
}

/// 构建托盘菜单：“打开”与“退出”三端通用；macOS 额外提供“划词录入”
/// （与全局快捷键等效，已设置快捷键时在菜单项中显示提示）；任一菜单项创建失败时
/// 返回 None（放弃本次创建/刷新，托盘保持原状）。
fn build_tray_menu(app: &AppHandle) -> Option<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};

    let mut items: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = Vec::new();

    // “划词录入”仅 macOS 提供（全局快捷键为 macOS 专属功能）。
    // 快捷键字符串作为菜单项加速键提示显示（macOS 渲染为 ⌘⇧S 样式），
    // 无法解析时由 Tauri 静默忽略（仅不显示提示，不影响菜单项）。
    #[cfg(target_os = "macos")]
    {
        let shortcut = read_config_or_default(app).global_shortcut().to_string();
        let accelerator = if shortcut.is_empty() { None } else { Some(shortcut.as_str()) };
        match MenuItem::with_id(app, "capture", "划词录入", true, accelerator) {
            Ok(item) => items.push(Box::new(item)),
            Err(error) => {
                log::warn!("failed to create tray menu item \"capture\": {error}");
                return None;
            }
        }
        match PredefinedMenuItem::separator(app) {
            Ok(item) => items.push(Box::new(item)),
            Err(error) => {
                log::warn!("failed to create tray menu separator: {error}");
                return None;
            }
        }
    }
    match MenuItem::with_id(app, "open", "打开 Anki 划词助手", true, None::<&str>) {
        Ok(item) => items.push(Box::new(item)),
        Err(error) => {
            log::warn!("failed to create tray menu item \"open\": {error}");
            return None;
        }
    }
    match MenuItem::with_id(app, "quit", "退出 Anki 划词助手", true, None::<&str>) {
        Ok(item) => items.push(Box::new(item)),
        Err(error) => {
            log::warn!("failed to create tray menu item \"quit\": {error}");
            return None;
        }
    }
    let item_refs: Vec<&dyn IsMenuItem<tauri::Wry>> =
        items.iter().map(|item| item.as_ref()).collect();
    return match Menu::with_items(app, &item_refs) {
        Ok(menu) => Some(menu),
        Err(error) => {
            log::warn!("failed to create tray menu: {error}");
            return None;
        }
    };
}

/// 注册托盘图标的点击事件处理（仅 Windows）：左键单击托盘图标直接打开主窗口
/// （Windows 惯例：左键激活、右键弹菜单，与 show_menu_on_left_click(false) 配合）。
/// macOS 左键弹菜单、Linux 的 appindicator 托盘不支持点击事件（任何点击均由系统
/// 弹菜单），“打开主窗口”走托盘菜单项，均无需处理点击。
#[cfg(target_os = "windows")]
pub fn register_tray_icon_event_handler(app: &AppHandle) {
    app.on_tray_icon_event(|app, event| {
        if let tauri::tray::TrayIconEvent::Click {
            button: tauri::tray::MouseButton::Left,
            button_state: tauri::tray::MouseButtonState::Up,
            ..
        } = event
        {
            if let Err(error) = super::shortcut::show_and_focus_main_window(app) {
                log::warn!("failed to show main window from tray icon click: {error}");
            }
        }
    });
}

/// 注册托盘菜单与 Dock 图标菜单共用的菜单事件处理（入口：打开/退出，macOS 另有划词录入）。
///
/// 在应用 setup 阶段注册一次。AppHandle::on_menu_event 接收所有 muda 菜单事件
/// （Tauri 把 muda 事件汇入事件循环后广播给应用级监听器），按菜单项 id 分发；
/// 不能依赖托盘存在——后台图标为 dock（仅 macOS）时托盘不会创建，但 Dock 菜单仍需可用。
pub fn register_menu_event_handler(app: &AppHandle) {
    app.on_menu_event(|app, event| match event.id.as_ref() {
        // 划词录入：与全局快捷键相同的捕获流程（读取选中文本后录入主窗口）。
        // 点击托盘/Dock 菜单不会激活本应用，选中文本仍来自用户当前所在的应用
        #[cfg(target_os = "macos")]
        "capture" => super::shortcut::on_shortcut_pressed(app.clone()),
        // 打开主窗口：复用划词快捷键/Dock 点击/托盘点击的显示并聚焦逻辑
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
