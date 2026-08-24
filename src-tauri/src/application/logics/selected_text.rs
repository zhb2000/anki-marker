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

/// macOS 辅助功能 C API（ApplicationServices 框架）的最小 FFI 绑定。
///
/// 仅绑定读取选中文本所需的符号，避免引入 `accessibility-ng`/`cocoa`
/// 整条依赖链（其中包含存在 future-incompat 问题的 `block` crate）。
///
/// AX 属性名不通过链接 C 全局符号获取（`kAX*Attribute` 等数据符号在
/// framework 的 tbd 导出中无法解析，会导致链接失败），而是直接使用其字符串值。
#[cfg(target_os = "macos")]
mod ax_ffi {
    use core_foundation::base::CFTypeRef;
    use core_foundation::string::CFStringRef;

    /// AXError 错误码，0 表示成功，负值为 HIServices 错误码。
    pub type AXError = i32;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        /// 创建系统级辅助功能元素，用于查询全局焦点元素。
        pub fn AXUIElementCreateSystemWide() -> CFTypeRef;
        /// 读取辅助功能元素的属性值（返回值已 retain，调用者负责释放）。
        pub fn AXUIElementCopyAttributeValue(
            element: CFTypeRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
    }

    /// 「当前焦点元素」属性名，即 kAXFocusedUIElementAttribute 的值。
    pub const FOCUSED_UI_ELEMENT_ATTRIBUTE: &str = "AXFocusedUIElement";
    /// 「选中文本」属性名，即 kAXSelectedTextAttribute 的值。
    pub const SELECTED_TEXT_ATTRIBUTE: &str = "AXSelectedText";
}

/// 通过辅助功能 API 读取当前焦点元素中的选中文本。
///
/// 注意：未授予辅助功能权限时此处会失败，由调用方回退或提示。
#[cfg(target_os = "macos")]
fn get_selected_text_by_ax() -> Result<String, String> {
    use core_foundation::base::{CFGetTypeID, CFType, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    unsafe {
        // AXUIElementCreateSystemWide 遵循 Create 规则（返回已 retain 的对象），
        // 包装成 CFType 借助 Drop 自动释放。
        let system_wide: CFType =
            TCFType::wrap_under_create_rule(ax_ffi::AXUIElementCreateSystemWide());

        let focused_attribute = CFString::from_static_string(ax_ffi::FOCUSED_UI_ELEMENT_ATTRIBUTE);
        let mut focused_ref: CFTypeRef = std::ptr::null();
        let err = ax_ffi::AXUIElementCopyAttributeValue(
            system_wide.as_CFTypeRef(),
            focused_attribute.as_concrete_TypeRef(),
            &mut focused_ref,
        );
        if err != 0 || focused_ref.is_null() {
            return Err(format!("failed to get focused UI element: AXError {err}"));
        }
        // AXUIElementCopyAttributeValue 遵循 Copy 规则（返回已 retain 的对象）。
        let focused: CFType = TCFType::wrap_under_create_rule(focused_ref);

        let selected_attribute = CFString::from_static_string(ax_ffi::SELECTED_TEXT_ATTRIBUTE);
        let mut text_ref: CFTypeRef = std::ptr::null();
        let err = ax_ffi::AXUIElementCopyAttributeValue(
            focused.as_CFTypeRef(),
            selected_attribute.as_concrete_TypeRef(),
            &mut text_ref,
        );
        if err != 0 || text_ref.is_null() {
            return Err(format!("failed to get selected text: AXError {err}"));
        }
        if CFGetTypeID(text_ref) != CFString::type_id() {
            return Err("selected text is not a string".to_string());
        }
        let selected_text: CFString =
            TCFType::wrap_under_create_rule(text_ref as CFStringRef);
        return Ok(selected_text.to_string());
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::get_selected_text_by_ax;

    /// 冒烟测试：FFI 调用路径不应 panic，结果取决于辅助功能权限与当前焦点元素。
    #[test]
    fn ax_ffi_smoke_test() {
        let result = get_selected_text_by_ax();
        eprintln!("get_selected_text_by_ax() -> {result:?}");
    }
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
