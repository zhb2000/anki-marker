//! 获取系统当前选中的文本（划词取词）。
//!
//! 各平台实现：
//! - macOS（`macos` 模块）：辅助功能 API（AXUIElement）优先，回退 AppleScript 模拟 Cmd+C；
//! - Windows（`windows` 模块）：UI Automation（TextPattern）优先，回退模拟 Ctrl+C 读剪贴板；
//! - Linux（`linux` 模块）：AT-SPI（D-Bus）优先，回退读 PRIMARY 选区。
//!
//! 三端的取句链路共享同一套纯逻辑（`capture` 模块）：平台层取得“选中的词”与
//! “选区周边的上下文窗口”及候选锚点偏移，由 capture 校验锚点并切句——
//! 宁可降级为仅录词，也不切出错误的句子。

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod capture;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub fn get_selected_context(
    word_to_sentence: bool,
    callbacks: RetryCallbacks,
) -> Result<SelectedContext, String> {
    return macos::get_selected_context(word_to_sentence, callbacks);
}

#[cfg(target_os = "windows")]
pub fn get_selected_context(
    word_to_sentence: bool,
    callbacks: RetryCallbacks,
) -> Result<SelectedContext, String> {
    return windows::get_selected_context(word_to_sentence, callbacks);
}

#[cfg(target_os = "linux")]
pub fn get_selected_context(
    word_to_sentence: bool,
    callbacks: RetryCallbacks,
) -> Result<SelectedContext, String> {
    return linux::get_selected_context(word_to_sentence, callbacks);
}

/// 后台重试的生命周期回调，供 UI 呈现"仍在补取完整句子"的状态。
///
/// 后台重试（目标应用的无障碍树异步物化时补发完整句子）对用户不可见，但补发结果会
/// 覆盖先到的降级结果，因此需要一对成对的状态信号驱动界面上的工作指示。
///
/// 调用约定：`on_started` 在重试线程真正开始轮询时调用一次；`on_finished` 在重试
/// 结束时调用恰好一次——`Some` 为补取到的结果，`None` 为超时放弃。重试未真正启动
/// 时（平台侧无重试依据，或已有重试在途被互斥守卫拦下）两个回调都不调用。
pub struct RetryCallbacks {
    pub on_started: Box<dyn FnOnce() + Send>,
    pub on_finished: Box<dyn FnOnce(Option<SelectedContext>) + Send>,
}

/// 划词捕获结果：text 为录入文本（取句成功时是整句），word 为取句模式命中的单词。
#[derive(Debug)]
pub struct SelectedContext {
    pub text: String,
    pub word: Option<String>,
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn get_selected_text() -> Result<String, String> {
    return Err("获取选中文本暂不支持此平台".to_string());
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn get_selected_context(
    _word_to_sentence: bool,
    _callbacks: RetryCallbacks,
) -> Result<SelectedContext, String> {
    return Err("获取选中文本暂不支持此平台".to_string());
}
