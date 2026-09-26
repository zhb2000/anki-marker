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
    on_retry_captured: impl FnOnce(SelectedContext) + Send + 'static,
) -> Result<SelectedContext, String> {
    return macos::get_selected_context(word_to_sentence, on_retry_captured);
}

#[cfg(target_os = "windows")]
pub fn get_selected_context(
    word_to_sentence: bool,
    on_retry_captured: impl FnOnce(SelectedContext) + Send + 'static,
) -> Result<SelectedContext, String> {
    return windows::get_selected_context(word_to_sentence, on_retry_captured);
}

#[cfg(target_os = "linux")]
pub fn get_selected_context(
    word_to_sentence: bool,
    on_retry_captured: impl FnOnce(SelectedContext) + Send + 'static,
) -> Result<SelectedContext, String> {
    return linux::get_selected_context(word_to_sentence, on_retry_captured);
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
    _on_retry_captured: impl FnOnce(SelectedContext) + Send + 'static,
) -> Result<SelectedContext, String> {
    return Err("获取选中文本暂不支持此平台".to_string());
}
