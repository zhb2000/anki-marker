//! 主窗口外观辅助。

use tauri::WebviewWindow;

#[cfg(target_os = "macos")]
use {
    objc2::rc::Retained,
    objc2::runtime::{AnyObject, Bool},
    objc2::{msg_send, ClassType},
    objc2_app_kit::{NSColor, NSWindow},
    objc2_foundation::{ns_string, NSNumber, NSProcessInfo},
    objc2_web_kit::WKWebView,
};

/// 把主窗口的背景色同步为应用主题底色（macOS）。
///
/// 两层背景都需要设置：
/// 1. NSWindow 背景色：透明标题栏（tauri.conf.json 中 titleBarStyle: Transparent）
///    露出的顶栏颜色，以及 zoom/resize 动画期间未被网页覆盖的窗口区域；
/// 2. WKWebView 底层背景：zoom 动画期间网页尚未按新尺寸合成时，WKWebView 自身
///    会用默认白色清底（双击标题栏最大化时的白闪即来源于此）。处理方式：
///    关闭 drawsBackground（私有 KVC，wry transparent 特性同款）让下层窗口底色透出，
///    并设置 underPageBackgroundColor（macOS 12+ 公开 API）同步页面背后底色。
///
/// 前端在主题应用/切换时调用（见 src/logics/theme.ts）；非 macOS 平台为 no-op。
/// 颜色取值与 src/fluent-controls/fluent-styles.css 的 --window-background 同步，
/// 修改那边时这里也要同步。
#[tauri::command]
pub fn set_window_background(window: WebviewWindow, dark: bool) {
    #[cfg(target_os = "macos")]
    {
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
        if NSProcessInfo::processInfo().operatingSystemVersion().majorVersion < 12 {
            return;
        }
        unsafe {
            let content_view: Retained<AnyObject> = msg_send![ns_window, contentView];
            sync_webview_background(&content_view, &color, 3);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, dark);
    }
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
        // 关闭 WKWebView 的"白色清底"行为（私有 KVC，wry transparent 特性同款做法，
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
