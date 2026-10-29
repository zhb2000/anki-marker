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

/// 单层遮罩的混合因子：底色按 (1 − MASK_ALPHA) 压暗（smoke 为纯黑半透明，预混即等比变暗）
#[cfg(target_os = "macos")]
const MASKED_FACTOR: f64 = 1.0 - MASK_ALPHA;

/// 叠层遮罩的深度上限：每层再乘一次单层因子，超过四层已接近全黑，再深没有视觉意义
#[cfg(target_os = "macos")]
const MAX_MASK_DEPTH: u32 = 4;

/// `depth` 层对话框叠起来时的目标混合因子。
/// 页面上的每层 FluentDialogShell 各渲染一层 smoke 遮罩，叠加后页面亮度为 (1 − alpha)^n，
/// 标题栏用同一公式，两层确认框叠在一层更新说明上时才会继续变深，不露馅。
#[cfg(target_os = "macos")]
fn mask_factor_for_depth(depth: u32) -> f64 {
    return MASKED_FACTOR.powi(depth.min(MAX_MASK_DEPTH) as i32);
}

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
/// 与 DOM 遮罩的进/出场过渡对齐。
///
/// `depth` 为当前打开的对话框层数（0 表示没有对话框）：页面上的每层对话框各叠一层
/// 遮罩，标题栏按同一公式逐层加深，嵌套确认框出现时不会停在单层深度。
/// 前端经 src/fluent-controls/dialogLayer.ts 的弹窗栈给出层数（见 logics/globals.ts 接线）；
/// 非 macOS 平台为 no-op。
#[tauri::command]
pub fn set_dialog_mask(window: WebviewWindow, depth: u32, dark: bool) {
    #[cfg(target_os = "macos")]
    start_mask_animation(&window, depth, dark);
    #[cfg(not(target_os = "macos"))]
    let _ = (window, depth, dark);
}

/// 启动遮罩因子动画：独立线程按帧间隔步进（AppKit 调用经 run_on_main_thread
/// 派发回主线程），每帧先更新 MASK_FACTOR 再着色，保证中途的 set_window_background
/// 读到的是最新因子。主题在动画中途切换时旧线程会以旧 dark 值播完剩余帧，
/// 随后前端主题切换流程会以新主题色 + 当前因子重新着色，最终一致。
#[cfg(target_os = "macos")]
fn start_mask_animation(window: &WebviewWindow, depth: u32, dark: bool) {
    let from = decode_factor(MASK_FACTOR.load(Ordering::Relaxed));
    let to = mask_factor_for_depth(depth);
    if (to - from).abs() < 1e-9 {
        return; // 已处于目标态（重复触发），无需动画
    }
    let duration = MASK_ANIM_DURATION;
    let generation = MASK_ANIM_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let window = window.clone();
    std::thread::spawn(move || {
        // 绝对时间节拍：每帧钉在 start + n × 帧间隔 的时刻上（sleep 到点而非“干完活再睡”），
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
            MASK_FACTOR.store(
                encode_factor(from + (to - from) * progress),
                Ordering::Relaxed,
            );
            let frame_window = window.clone();
            if window
                .run_on_main_thread(move || apply_window_background(&frame_window, dark))
                .is_err()
            {
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{mask_factor_for_depth, MAX_MASK_DEPTH};

    /// 每多一层对话框，标题栏按单层遮罩因子再压一次（与 DOM 叠加遮罩一致）；深度有上限
    #[test]
    fn mask_factor_scales_with_depth() {
        assert!((mask_factor_for_depth(0) - 1.0).abs() < 1e-9);
        let one = mask_factor_for_depth(1);
        assert!((one - 178.0 / 255.0).abs() < 1e-9);
        assert!((mask_factor_for_depth(2) - one * one).abs() < 1e-12);
        assert_eq!(
            mask_factor_for_depth(9),
            mask_factor_for_depth(MAX_MASK_DEPTH)
        );
    }
}
