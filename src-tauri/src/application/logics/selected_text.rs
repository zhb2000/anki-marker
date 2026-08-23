//! 获取系统当前选中的文本（目前仅 macOS）。
//!
//! 实现参考了 yetone/get-selected-text（MIT / Apache-2.0）的 macOS 部分：
//! 优先通过辅助功能 API（AXUIElement）直接读取选中文字；
//! 目标应用不兼容时回退为执行 AppleScript——临时静音系统提示音、模拟按下
//! Cmd+C 读取剪贴板后恢复剪贴板与音量。
//! 两条路径均要求在「系统设置 → 隐私与安全性 → 辅助功能」中授权本应用。

#[cfg(target_os = "macos")]
pub fn get_selected_text() -> Result<String, String> {
    if let Ok(text) = get_selected_text_by_ax() {
        return Ok(text);
    }
    return get_selected_text_by_applescript();
}

#[cfg(not(target_os = "macos"))]
pub fn get_selected_text() -> Result<String, String> {
    return Err("获取选中文本暂不支持此平台".to_string());
}

/// 通过辅助功能 API 读取当前焦点元素中的选中文本。
///
/// 注意：未授予辅助功能权限时此处会失败，由调用方回退或提示。
#[cfg(target_os = "macos")]
fn get_selected_text_by_ax() -> Result<String, String> {
    use core_foundation::string::CFString;

    use accessibility_ng::{AXAttribute, AXUIElement};
    use accessibility_sys_ng::{kAXFocusedUIElementAttribute, kAXSelectedTextAttribute};

    let system_element = AXUIElement::system_wide();
    let focused_element = system_element
        .attribute(&AXAttribute::new(&CFString::from_static_string(
            kAXFocusedUIElementAttribute,
        )))
        .map_err(|e| format!("failed to get focused UI element: {e:?}"))?
        .downcast_into::<AXUIElement>()
        .ok_or("focused UI element is not an AXUIElement")?;
    let selected_text = focused_element
        .attribute(&AXAttribute::new(&CFString::from_static_string(
            kAXSelectedTextAttribute,
        )))
        .map_err(|e| format!("failed to get selected text: {e:?}"))?
        .downcast_into::<CFString>()
        .ok_or("selected text is not a string")?;
    return Ok(selected_text.to_string());
}

/// 模拟 Cmd+C 读取剪贴板的 AppleScript：
/// 备份剪贴板 → 临时静音系统提示音 → 模拟 Cmd+C → 读取剪贴板 → 恢复剪贴板与音量。
///
/// 已知限制：若剪贴板当前存放的不是文本（如图片），备份步骤会失败，整个回退随之失败。
#[cfg(target_os = "macos")]
const APPLE_SCRIPT: &str = r#"
use AppleScript version "2.4"
use scripting additions
use framework "Foundation"
use framework "AppKit"

set savedAlertVolume to alert volume of (get volume settings)

-- Back up clipboard contents:
set savedClipboard to the clipboard

set thePasteboard to current application's NSPasteboard's generalPasteboard()
set theCount to thePasteboard's changeCount()

tell application "System Events"
    set volume alert volume 0
end tell

-- Copy selected text to clipboard:
tell application "System Events" to keystroke "c" using {command down}
delay 0.1 -- Without this, the clipboard may have stale data.

tell application "System Events"
    set volume alert volume savedAlertVolume
end tell

if thePasteboard's changeCount() is theCount then
    return ""
end if

set theSelectedText to the clipboard

set the clipboard to savedClipboard

theSelectedText
"#;

/// 通过 AppleScript 模拟 Cmd+C 并读取剪贴板，获取当前选中的文本。
#[cfg(target_os = "macos")]
fn get_selected_text_by_applescript() -> Result<String, String> {
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(APPLE_SCRIPT)
        .output()
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    if output.status.success() {
        let content = String::from_utf8(output.stdout)
            .map_err(|e| format!("osascript output is not valid utf-8: {e}"))?;
        return Ok(content.trim().to_string());
    }
    let error = String::from_utf8_lossy(&output.stderr).into_owned();
    return Err(format!("osascript failed: {error}"));
}
