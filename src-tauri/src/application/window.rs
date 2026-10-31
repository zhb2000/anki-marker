//! 主窗口的显示与重建、外观与 macOS 标题栏行为。
//!
//! 窗口使用 `titleBarStyle: "Overlay"`（见 tauri.conf.json）：原生标题栏透明、内容视图
//! 铺满整个窗口，红绿灯浮在网页之上。这样弹窗遮罩（DOM 中 position: fixed 的 smoke）
//! 能自然覆盖到标题栏区域，不再需要从 Rust 侧逐帧模拟标题栏配色。
//!
//! 代价是窗口的拖动与双击手势需要自己接管（隐藏标题后 AppKit 不再处理标题栏双击）：
//! - 拖动：前端在顶部标题栏高度的命中条上调用 start_dragging（见 components/MacTitleBar.vue）；
//! - 双击：Rust 侧装一个原生事件监听，按系统偏好执行缩放/最小化/无操作。

use tauri::{AppHandle, Manager, WebviewWindow};

#[cfg(target_os = "macos")]
use {
    block2::RcBlock,
    objc2::rc::{Retained, Weak},
    objc2::runtime::{AnyObject, Bool},
    objc2::{msg_send, ClassType},
    objc2_app_kit::{NSColor, NSEvent, NSEventMask, NSEventType, NSWindow, NSWindowButton},
    objc2_foundation::{ns_string, NSNumber, NSPoint, NSProcessInfo, NSUserDefaults},
    objc2_web_kit::WKWebView,
    std::cell::{Cell, RefCell},
    std::ptr::NonNull,
    std::sync::Mutex,
};

/// 显示并聚焦主窗口；若主窗口已不存在，则按 tauri.conf.json 中的窗口配置重建。
///
/// 调用时机：划词快捷键触发（macOS）、点击 Dock 图标（macOS）、点击托盘图标/托盘
/// 菜单“打开”（Windows/Linux）。后台运行期间点关闭按钮仅隐藏窗口，正常情况下
/// 窗口始终存在，重建仅作兜底。
pub fn show_and_focus_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        // 从最小化状态恢复（show() 对已最小化的窗口不自动还原）
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        // 恢复 macOS 的 Dock 图标（托盘图标常驻，不受窗口显隐影响）
        super::menubar::on_main_window_shown(app);
        return Ok(());
    }
    let window_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
        .ok_or("main window config not found")?;
    let window = tauri::WebviewWindowBuilder::from_config(app, &window_config)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    // tauri.conf.json 中窗口初始 visible: false（防启动闪屏），此处直接显示；
    // 前端就绪后的 show() 是幂等的
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    // 新窗口没有 setup 阶段挂在旧窗口上的原生监听（macOS 标题栏双击、Windows 任务栏
    // 深浅子类化），重新挂载
    install_main_window_hooks(app);
    // 恢复 macOS 的 Dock 图标（托盘图标常驻，不受窗口显隐影响）
    super::menubar::on_main_window_shown(app);
    return Ok(());
}

/// 安装主窗口的原生挂载：macOS 标题栏双击监听、Windows 任务栏深浅监听（子类化主窗口）。
///
/// 应用 setup 阶段调用一次；主窗口被销毁重建后（见 show_and_focus_main_window）
/// 必须对新窗口重新挂载，否则新窗口没有这些行为。可从任意线程调用。
pub fn install_main_window_hooks(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if let Some(window) = app.get_webview_window("main") {
        install_titlebar_monitor(&window);
    }
    #[cfg(target_os = "windows")]
    {
        // SetWindowSubclass 不能跨线程子类化窗口，必须在窗口所属的主线程上调用
        let handle = app.clone();
        if let Err(error) =
            app.run_on_main_thread(move || super::menubar::install_tray_theme_listener(&handle))
        {
            log::warn!("failed to install the taskbar theme listener: {error}");
        }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = app;
    }
}

/// 双击标题栏时第二次按下的位置（窗口坐标），用于判断按下后是否移动过：
/// 与 AppKit 的行为一致，双击后拖动鼠标应取消缩放。
#[cfg(target_os = "macos")]
static DOUBLE_CLICK_ORIGIN: Mutex<Option<(f64, f64)>> = Mutex::new(None);

// 事件监听回调与安装都只在主线程执行（AppKit 限制），故用线程局部变量
#[cfg(target_os = "macos")]
thread_local! {
    /// 标题栏监听作用的主窗口。只持弱引用：主窗口被销毁重建后旧 NSWindow 随之释放，
    /// 弱引用自动失效，监听里不会解引用悬垂指针；重建后由 install_titlebar_monitor 换成新窗口
    static TITLEBAR_TARGET: RefCell<Option<Weak<NSWindow>>> = const { RefCell::new(None) };
    /// 事件监听只装一次（全局生效），窗口重建时只需更新 TITLEBAR_TARGET
    static TITLEBAR_MONITOR_INSTALLED: Cell<bool> = const { Cell::new(false) };
}

/// 安装标题栏双击监听（macOS）。
///
/// 窗口是 Overlay 标题栏 + hiddenTitle，AppKit 会跳过标题栏的默认双击手势
/// （见 https://stackoverflow.com/questions/52150960），因此这里接管：
/// 双击落在标题栏矩形内、且不在红绿灯按钮上时，按系统偏好执行对应动作。
/// 事件被吞掉（返回 null），避免同一次双击再被网页或 AppKit 处理一次。
///
/// 用本地事件监听而不是网页里的 dblclick，是为了覆盖网页收不到事件的区域
/// （原生标题栏视图在上层）以及窗口未激活时的第一次点击。
#[cfg(target_os = "macos")]
pub fn install_titlebar_monitor(window: &WebviewWindow) {
    // AppKit 对象只能在主线程访问；窗口重建后本函数可能从其它线程调用，转发到主线程
    if objc2::MainThreadMarker::new().is_none() {
        let target = window.clone();
        if let Err(error) = window.run_on_main_thread(move || install_titlebar_monitor(&target)) {
            log::warn!("failed to install the titlebar monitor: {error}");
        }
        return;
    }
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    // 此刻窗口存活，指针有效；retain 后只留弱引用
    let Some(ns_window) = (unsafe { Retained::retain(ns_window as *mut NSWindow) }) else {
        return;
    };
    TITLEBAR_TARGET.with(|target| *target.borrow_mut() = Some(Weak::from_retained(&ns_window)));
    if TITLEBAR_MONITOR_INSTALLED.with(|installed| installed.replace(true)) {
        return;
    }

    let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let raw = event.as_ptr();
        let event = unsafe { event.as_ref() };
        // 只处理主窗口；调试窗口等其它窗口保持系统行为。主窗口已释放时弱引用为 None，放行
        let own_window =
            TITLEBAR_TARGET.with(|target| target.borrow().as_ref().and_then(Weak::load));
        let Some(own_window) = own_window else {
            return raw;
        };
        if event.windowNumber() != own_window.windowNumber() || event.clickCount() < 2 {
            return raw;
        }
        let window = &*own_window;
        let point = event.locationInWindow();

        match event.r#type() {
            NSEventType::LeftMouseDown => {
                // 第二次按下落在标题栏内才接管；红绿灯自己响应双击，放行
                if !is_in_titlebar(window, point) || is_over_window_button(window, point) {
                    return raw;
                }
                *DOUBLE_CLICK_ORIGIN.lock().unwrap() = Some((point.x, point.y));
                std::ptr::null_mut() // 吞掉第二次按下，避免又触发一次拖拽
            }
            NSEventType::LeftMouseUp => {
                // 只认“上一次按下已被接管”的抬起，避免误伤其它双击
                let origin = DOUBLE_CLICK_ORIGIN.lock().unwrap().take();
                let Some((x, y)) = origin else {
                    return raw;
                };
                // 双击后移动了鼠标：取消，与原生标题栏一致
                if (point.x - x).abs() <= 2.0 && (point.y - y).abs() <= 2.0 {
                    perform_double_click_action(window);
                }
                std::ptr::null_mut()
            }
            _ => raw,
        }
    });

    // 监听对象释放即失效；监听只装一次、与进程同生命周期，直接泄漏住
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::LeftMouseDown | NSEventMask::LeftMouseUp,
            &handler,
        )
    };
    if let Some(monitor) = monitor {
        std::mem::forget(monitor);
    }
    // 监听对象与 block 一起泄漏住：即便 AppKit 未额外持有 block，也不会悬垂
    std::mem::forget(handler);
}

/// 点是否落在标题栏区域内（窗口坐标，原点在左下角）。
/// 全屏时 contentLayoutRect 与窗口内容区重合，此处自然返回 false。
#[cfg(target_os = "macos")]
fn is_in_titlebar(window: &NSWindow, point: NSPoint) -> bool {
    let layout = window.contentLayoutRect();
    let top = window.frame().size.height;
    point.y >= layout.origin.y + layout.size.height && point.y <= top
}

/// 点是否落在红绿灯按钮上：这些按钮自己响应双击（例如双击绿色按钮进入全屏），
/// 不能被标题栏监听截走。
#[cfg(target_os = "macos")]
fn is_over_window_button(window: &NSWindow, point: NSPoint) -> bool {
    for kind in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        let Some(button) = window.standardWindowButton(kind) else {
            continue;
        };
        // objc2-core-foundation 未导出 CGRectContainsPoint，直接比较边界
        let rect = button.convertRect_toView(button.bounds(), None);
        if point.x >= rect.origin.x
            && point.x <= rect.origin.x + rect.size.width
            && point.y >= rect.origin.y
            && point.y <= rect.origin.y + rect.size.height
        {
            return true;
        }
    }
    false
}

/// 按系统偏好“双击窗口标题栏时”执行：缩放（默认）/ 最小化 / 无操作。
#[cfg(target_os = "macos")]
fn perform_double_click_action(window: &NSWindow) {
    let action = NSUserDefaults::standardUserDefaults()
        .stringForKey(ns_string!("AppleActionOnDoubleClick"))
        .map(|value| value.to_string());
    match action.as_deref() {
        Some("Minimize") => window.performMiniaturize(None),
        Some("None") => {}
        _ => window.performZoom(None),
    }
}

/// 把主窗口的背景色同步为应用主题底色（macOS）。
///
/// 内容视图虽然铺满窗口，但窗口缩放动画期间网页尚未按新尺寸合成时，会露出
/// WKWebView / NSWindow 的底色。两层都要设置：
/// 1. NSWindow 背景色：窗口缩放等未被网页覆盖处；
/// 2. WKWebView 底层背景：关闭 drawsBackground（私有 KVC，wry transparent 特性同款）
///    让下层窗口底色透出，并设置 underPageBackgroundColor（macOS 12+ 公开 API）。
///
/// 前端在主题应用/切换时调用（见 src/logics/theme.ts）；非 macOS 平台为 no-op。
/// 颜色取值与 src/fluent-controls/fluent-styles.css 的 --window-background 同步，
/// 修改那边时这里也要同步。
#[tauri::command]
pub fn set_window_background(window: WebviewWindow, dark: bool) {
    apply_window_background(&window, dark);
}

#[cfg(target_os = "macos")]
fn apply_window_background(window: &WebviewWindow, dark: bool) {
    let (red, green, blue) = if dark {
        (0x20, 0x20, 0x20)
    } else {
        (0xf3, 0xf3, 0xf3)
    };
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = unsafe { &*(ns_window as *mut NSWindow) };
    // colorWithSRGBRed / setBackgroundColor 在 objc2-app-kit 中为安全方法
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from(red) / 255.0,
        f64::from(green) / 255.0,
        f64::from(blue) / 255.0,
        1.0,
    );
    ns_window.setBackgroundColor(Some(&color));

    // underPageBackgroundColor 需要 macOS 12+（drawsBackground 私有 KVC 为 macOS 10.14+）
    if NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion
        < 12
    {
        return;
    }
    unsafe {
        let content_view: Retained<AnyObject> = msg_send![ns_window, contentView];
        sync_webview_background(&content_view, &color, 3);
    }
}

#[cfg(not(target_os = "macos"))]
fn apply_window_background(window: &WebviewWindow, dark: bool) {
    let _ = (window, dark);
}

/// 在视图层级中查找 WKWebView 并同步其底层背景行为。
///
/// wry 的视图层级为 NSWindow contentView -> 容器 NSView -> WKWebView，
/// 逐层向下搜索（depth 限制层数），命中即返回（WKWebView 内部不再是普通视图层级）。
#[cfg(target_os = "macos")]
unsafe fn sync_webview_background(view: &AnyObject, color: &NSColor, depth: u32) {
    let is_web_view: Bool = msg_send![view, isKindOfClass: WKWebView::class()];
    if is_web_view.as_bool() {
        let web_view = &*(view as *const AnyObject as *const WKWebView);
        // 关闭 WKWebView 的“白色清底”行为（私有 KVC，wry transparent 特性同款做法，
        // 见 https://stackoverflow.com/questions/27655930）：
        // zoom/resize 动画期间网页尚未按新尺寸合成时，下层 NSWindow 的主题底色
        // 直接透出，不再露出 WKWebView 默认的白色。
        // objc2-web-kit 未暴露 setValue_forKey，用 msg_send 发送同一 selector
        let no = NSNumber::numberWithBool(false);
        // Apple 头文件将该 selector 标记为 deprecated，但 KVC 机制仍在运行时支持
        // （wry 的 transparent 特性同样调用它），故局部抑制弃用警告
        #[allow(deprecated)]
        let _: () = msg_send![web_view, setValue: &*no forKey: ns_string!("drawsBackground")];
        // macOS 12+ 公开 API：页面背后的颜色（滚动越界等场景）
        web_view.setUnderPageBackgroundColor(Some(color));
        return;
    }
    if depth == 0 {
        return;
    }
    let subviews: Retained<AnyObject> = msg_send![view, subviews];
    let count: usize = msg_send![&subviews, count];
    for index in 0..count {
        let subview: Retained<AnyObject> = msg_send![&subviews, objectAtIndex: index];
        sync_webview_background(&subview, color, depth - 1);
    }
}
