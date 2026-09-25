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
    std::sync::atomic::{AtomicU64, Ordering},
    std::time::{Duration, Instant},
};

/// 对话框遮罩（FluentDialogShell 的 smoke 背板 #0000004D）的 alpha。
/// 与 src/fluent-controls/FluentDialogShell.vue 的 --dialog-smoke-color 同步，修改那边时这里也要同步。
#[cfg(target_os = "macos")]
const MASK_ALPHA: f64 = 0x4D as f64 / 255.0;

/// 遮罩态的目标混合因子：底色按 (1 − MASK_ALPHA) 压暗（smoke 为纯黑半透明，预混即等比变暗）
#[cfg(target_os = "macos")]
const MASKED_FACTOR: f64 = 1.0 - MASK_ALPHA;

/// 动画时长对齐 DOM 遮罩过渡（FluentDialogShell.vue 的 .fluent-dialog-enter/leave-active）：
/// 遮罩（smoke）本体淡入与淡出均为 0.167s linear（0.25s 是容器缩放动画的时长，与本处无关）
#[cfg(target_os = "macos")]
const MASK_ANIM_DURATION: Duration = Duration::from_millis(167);
/// 帧间隔 ≈ 120fps：步进插值的量化台阶减半，与 vsync 驱动的 DOM 遮罩过渡更接近
#[cfg(target_os = "macos")]
const MASK_ANIM_FRAME_INTERVAL: Duration = Duration::from_millis(8);

/// 当前底色亮度系数（MASKED_FACTOR ~ 1.0：1.0 为原色，MASKED_FACTOR 为遮罩态，
/// 以百万分之一为单位存整数）。
/// set_dialog_mask 的动画线程写、set_window_background 读：主题切换若发生在
/// 展示/动画期间，按当前系数着色，标题栏不会跳出未混合的原色。
#[cfg(target_os = "macos")]
static MASK_FACTOR: AtomicU64 = AtomicU64::new(1_000_000);
/// 动画代际号：每次触发新动画自增，旧线程发现代际不符即自行退出（快速开关时取消旧动画）
#[cfg(target_os = "macos")]
static MASK_ANIM_GENERATION: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
fn encode_factor(factor: f64) -> u64 {
    (factor * 1_000_000.0).round() as u64
}

#[cfg(target_os = "macos")]
fn decode_factor(raw: u64) -> f64 {
    raw as f64 / 1_000_000.0
}

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
    apply_window_background(&window, dark);
}

/// 对话框遮罩态的标题栏同步（macOS）。
///
/// DOM 遮罩只覆盖 webview，透明标题栏露出的原生顶栏不受其影响；backgroundColor
/// 不是 animator 可动画属性，这里用动画线程逐帧插值混合因子并派发到主线程重设，
/// 与 DOM 遮罩的进/出场过渡对齐。前端在对话框开/关时调用
/// （见 src/fluent-controls/ContentDialog.ts 的 setDialogOpenListener 接线）；
/// 非 macOS 平台为 no-op。
#[tauri::command]
pub fn set_dialog_mask(window: WebviewWindow, active: bool, dark: bool) {
    #[cfg(target_os = "macos")]
    start_mask_animation(&window, active, dark);
    #[cfg(not(target_os = "macos"))]
    let _ = (window, active, dark);
}

/// 启动遮罩因子动画：独立线程按帧间隔步进（AppKit 调用经 run_on_main_thread
/// 派发回主线程），每帧先更新 MASK_FACTOR 再着色，保证中途的 set_window_background
/// 读到的是最新因子。主题在动画中途切换时旧线程会以旧 dark 值播完剩余帧，
/// 随后前端主题切换流程会以新主题色 + 当前因子重新着色，最终一致。
#[cfg(target_os = "macos")]
fn start_mask_animation(window: &WebviewWindow, active: bool, dark: bool) {
    let from = decode_factor(MASK_FACTOR.load(Ordering::Relaxed));
    let to = if active { MASKED_FACTOR } else { 1.0 };
    if (to - from).abs() < 1e-9 {
        return; // 已处于目标态（重复触发），无需动画
    }
    let duration = MASK_ANIM_DURATION;
    let generation = MASK_ANIM_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let window = window.clone();
    std::thread::spawn(move || {
        // 绝对时间节拍：每帧钉在 start + n × 帧间隔 的时刻上（sleep 到点而非"干完活再睡"），
        // 帧耗时（IPC 派发等）不累积进节拍，动画总时长与 DOM 过渡严格一致；
        // 超过 end 的帧收拢到 end，保证终止帧精确落在动画时长上
        let started = Instant::now();
        let end = started + duration;
        let mut next_deadline = started + MASK_ANIM_FRAME_INTERVAL;
        loop {
            if MASK_ANIM_GENERATION.load(Ordering::Relaxed) != generation {
                return; // 已被更新的动画取代
            }
            let now = Instant::now();
            if next_deadline > now {
                std::thread::sleep(next_deadline - now);
            }
            if MASK_ANIM_GENERATION.load(Ordering::Relaxed) != generation {
                return; // 睡眠期间被更新的动画取代
            }
            let progress = (started.elapsed().as_secs_f64() / duration.as_secs_f64()).min(1.0);
            MASK_FACTOR.store(encode_factor(from + (to - from) * progress), Ordering::Relaxed);
            let frame_window = window.clone();
            if window.run_on_main_thread(move || apply_window_background(&frame_window, dark)).is_err() {
                return; // 窗口已销毁
            }
            if progress >= 1.0 {
                return;
            }
            next_deadline += MASK_ANIM_FRAME_INTERVAL;
            let now = Instant::now();
            if next_deadline > end {
                next_deadline = end;
            } else if next_deadline < now {
                // 长帧落后时顺延到当前时刻之后，避免追帧突发
                next_deadline = now + MASK_ANIM_FRAME_INTERVAL;
            }
        }
    });
}

/// 按当前遮罩因子着色（因子由动画线程维护，未动画时为 0.0 或目标值）
#[cfg(target_os = "macos")]
fn apply_window_background(window: &WebviewWindow, dark: bool) {
    let factor = decode_factor(MASK_FACTOR.load(Ordering::Relaxed));
    apply_window_background_with_factor(window, dark, factor);
}

#[cfg(target_os = "macos")]
fn apply_window_background_with_factor(window: &WebviewWindow, dark: bool, factor: f64) {
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
        f64::from(red) * factor / 255.0,
        f64::from(green) * factor / 255.0,
        f64::from(blue) * factor / 255.0,
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
