//! 自启动辅助（macOS 专属）：登录自启动的应用级隐藏状态检测与解除。
//!
//! autostart 插件的 AppleScript 登录项模式把 `--hidden` 参数映射为登录项自身的
//! hidden 标志（该模式不向 argv 传参）。登录时 LaunchServices 携 hidden 标志拉起
//! 应用，进程自启动起即处于 NSApplication 级隐藏状态——argv 中无任何标记，
//! 无法从启动参数识别，只能查询 NSApplication.isHidden 判定。
//!
//! 应用级隐藏会拦截后续所有窗口显示与激活（托盘/Dock/快捷键等唤起路径失效），
//! 故在应用 setup 阶段检测并解除（unhideWithoutActivation：不激活、不置前，
//! 不影响主窗口保持隐藏的静默启动语义，是否静默由调用方结合配置决定）。
//!
//! 时序说明：LaunchServices 的 hidden 启动等价于进程创建即带隐藏标记，
//! NSApp.isHidden 在 setup 阶段（事件循环启动前）已为 true。

/// 应用当前是否处于应用级隐藏状态（登录自启动 hidden 拉起的判据）。
///
/// 仅可在主线程调用（NSApplication 限制）。
#[cfg(target_os = "macos")]
pub fn is_app_hidden() -> bool {
    // 应用 setup 阶段运行在主线程（Tauri 事件循环启动前），此处必然是 Some
    let mtm = objc2::MainThreadMarker::new().expect("must be called on the main thread");
    objc2_app_kit::NSApplication::sharedApplication(mtm).isHidden()
}

/// 解除应用级隐藏，不激活应用、不显示任何窗口。
///
/// 仅可在主线程调用（NSApplication 限制）。
#[cfg(target_os = "macos")]
pub fn unhide_app_without_activation() {
    // 同 is_app_hidden：调用方位于应用 setup 阶段的主线程
    let mtm = objc2::MainThreadMarker::new().expect("must be called on the main thread");
    objc2_app_kit::NSApplication::sharedApplication(mtm).unhideWithoutActivation()
}
