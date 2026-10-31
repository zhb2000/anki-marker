//! macOS 辅助功能 C API（ApplicationServices 框架）的最小 FFI 绑定。
//!
//! 仅绑定读取选中文本与选词取句所需的符号，避免引入 `accessibility-ng`/`cocoa`
//! 整条依赖链（其中包含存在 future-incompat 问题的 `block` crate）。
//!
//! AX 属性名不通过链接 C 全局符号获取（`kAX*Attribute` 等数据符号在
//! framework 的 tbd 导出中无法解析，会导致链接失败），而是直接使用其字符串值。

use core_foundation::base::CFTypeRef;
use core_foundation::string::CFStringRef;

/// AXError 错误码，0 表示成功，负值为 HIServices 错误码。
pub type AXError = i32;

/// AXValue 包装的结构类型标签。SDK 头文件 AXValue.h 中为
/// CF_ENUM(UInt32, AXValueType)，按 4 字节整型传递。
pub type AXValueType = u32;

/// 包装 CFRange 的 AXValue 类型标签，即 kAXValueTypeCFRange（旧名 kAXValueCFRangeType）。
pub const AX_VALUE_CF_RANGE_TYPE: AXValueType = 4;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// 创建系统级辅助功能元素，用于查询全局焦点元素。
    pub fn AXUIElementCreateSystemWide() -> CFTypeRef;
    /// 创建指定进程 id 的应用的辅助功能元素（Create 规则）。
    pub fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    /// 读取元素所属应用的进程 id。
    pub fn AXUIElementGetPid(element: CFTypeRef, pid: *mut i32) -> AXError;
    /// 设置跨进程消息的超时（秒）。传系统级元素时对本进程全局生效，
    /// 传其他元素时只对该元素对象生效（见 SDK AXUIElement.h）。
    pub fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout_in_seconds: f32) -> AXError;
    /// 读取辅助功能元素的属性值（返回值已 retain，调用者负责释放）。
    pub fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    /// 设置辅助功能元素的属性值（用于设置 AXManualAccessibility 等唤醒开关）。
    pub fn AXUIElementSetAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    /// 读取参数化属性的值（Copy 规则，返回值已 retain，调用者负责释放）。
    pub fn AXUIElementCopyParameterizedAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    /// AXTextMarkerRange 的类型 id（解包前校验用，见 ensure_text_marker_range）。
    pub fn AXTextMarkerRangeGetTypeID() -> usize;
    /// 取 AXTextMarkerRange 的起点 marker（Copy 规则，返回值已 retain，调用者
    /// 负责释放）。AXTextMarker 体系公开 C API，随 HIServices 导出。
    pub fn AXTextMarkerRangeCopyStartMarker(range: CFTypeRef) -> CFTypeRef;
    /// 取 AXTextMarkerRange 的终点 marker（规则同上）。
    pub fn AXTextMarkerRangeCopyEndMarker(range: CFTypeRef) -> CFTypeRef;
    /// 创建包装 C 结构体的 AXValue（Create 规则，返回值已 retain，调用者负责释放）。
    pub fn AXValueCreate(the_type: AXValueType, value_ptr: *const std::ffi::c_void) -> CFTypeRef;
    /// 从 AXValue 解包 C 结构体到 value_ptr（Boolean 返回，非 0 表示成功）。
    pub fn AXValueGetValue(
        the_value: CFTypeRef,
        the_type: AXValueType,
        value_ptr: *mut std::ffi::c_void,
    ) -> u8;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    /// 数组元素个数（CFArrayGetCount；CFArrayRef 与 CFTypeRef ABI 兼容）。
    pub fn CFArrayGetCount(the_array: CFTypeRef) -> isize;
    /// 按下标取数组元素（CFArrayGetValueAtIndex；Get 规则，元素借用自数组，
    /// 数组存活期间有效，调用者不得释放）。
    pub fn CFArrayGetValueAtIndex(the_array: CFTypeRef, idx: isize) -> CFTypeRef;
    /// CFAttributedString 的类型 id（AX 的 AXAttributedStringForRange 返回此类型）。
    pub fn CFAttributedStringGetTypeID() -> usize;
    /// 取富文本的纯文本部分（Get 规则：借用自富文本对象，不得释放）。
    pub fn CFAttributedStringGetString(a_str: CFTypeRef) -> CFStringRef;
}

/// 硬件层面的键盘修饰键状态，即 kCGEventSourceStateHIDSystemState：反映物理按键，
/// 不受其他进程合成事件的影响。
pub const CG_EVENT_SOURCE_STATE_HID_SYSTEM: i32 = 1;
/// Shift / Control / Option / Command 的 CGEventFlags 掩码（kCGEventFlagMask*）。
/// 不含 Caps Lock：它是锁定键，长期置位，且不会与模拟按键合并成别的快捷键。
pub const CG_EVENT_FLAG_MASK_MODIFIERS: u64 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000 | 0x0010_0000;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    /// 读取当前修饰键状态（CGEventFlags 位掩码）。
    pub fn CGEventSourceFlagsState(state_id: i32) -> u64;
}

/// “当前焦点元素”属性名，即 kAXFocusedUIElementAttribute 的值。
pub const FOCUSED_UI_ELEMENT_ATTRIBUTE: &str = "AXFocusedUIElement";
/// “选中文本”属性名，即 kAXSelectedTextAttribute 的值。
pub const SELECTED_TEXT_ATTRIBUTE: &str = "AXSelectedText";
/// “所在窗口”属性名，即 kAXWindowAttribute 的值。
pub const WINDOW_ATTRIBUTE: &str = "AXWindow";
/// “当前焦点应用”属性名，即 kAXFocusedApplicationAttribute 的值。
pub const FOCUSED_APPLICATION_ATTRIBUTE: &str = "AXFocusedApplication";
/// “当前焦点窗口”属性名，即 kAXFocusedWindowAttribute 的值。
pub const FOCUSED_WINDOW_ATTRIBUTE: &str = "AXFocusedWindow";
/// “子元素列表”属性名，即 kAXChildrenAttribute 的值。
pub const CHILDREN_ATTRIBUTE: &str = "AXChildren";
/// “手动激活无障碍”属性名（AXManualAccessibility，Electron/Chromium 系应用
/// 监听此属性被设置来激活无障碍树）。
pub const MANUAL_ACCESSIBILITY_ATTRIBUTE: &str = "AXManualAccessibility";
/// “增强用户界面”属性名（AXEnhancedUserInterface，另一类应用使用的激活开关）。
pub const ENHANCED_USER_INTERFACE_ATTRIBUTE: &str = "AXEnhancedUserInterface";
/// “选区范围”属性名，即 kAXSelectedTextRangeAttribute 的值。
pub const SELECTED_TEXT_RANGE_ATTRIBUTE: &str = "AXSelectedTextRange";
/// “按范围取子串”参数化属性名，即 kAXStringForRangeParameterizedAttribute 的值。
pub const STRING_FOR_RANGE_PARAMETERIZED_ATTRIBUTE: &str = "AXStringForRange";
/// “文本字符数”属性名，即 kAXNumberOfCharactersAttribute 的值。
pub const NUMBER_OF_CHARACTERS_ATTRIBUTE: &str = "AXNumberOfCharacters";
/// 包装 CGRect 的 AXValue 类型标签，即 kAXValueTypeCGRect。
pub const AX_VALUE_CG_RECT_TYPE: AXValueType = 3;
/// “角色”属性名，即 kAXRoleAttribute 的值。
pub const ROLE_ATTRIBUTE: &str = "AXRole";
/// “父元素”属性名，即 kAXParentAttribute 的值。
pub const PARENT_ATTRIBUTE: &str = "AXParent";
/// “屏幕坐标系下的外框”属性名，即 kAXFrameAttribute 的值。
pub const FRAME_ATTRIBUTE: &str = "AXFrame";
/// 按范围取富文本的参数化属性名（PDFKit 的 AXPage 元素用它暴露整页文本）。
pub const ATTRIBUTED_STRING_FOR_RANGE_PARAMETERIZED_ATTRIBUTE: &str = "AXAttributedStringForRange";
/// “选区 marker 范围”属性名（AXTextMarker 体系：Safari 网页、Chromium 系等提供方
/// 不提供 AXSelectedText / AXSelectedTextRange，选区只经此属性暴露）。
pub const SELECTED_TEXT_MARKER_RANGE_ATTRIBUTE: &str = "AXSelectedTextMarkerRange";
/// 按 marker 范围取子串的参数化属性名。
pub const STRING_FOR_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE: &str = "AXStringForTextMarkerRange";
/// 取 marker 所在段落的 marker 范围的参数化属性名。
pub const PARAGRAPH_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE: &str =
    "AXParagraphTextMarkerRangeForTextMarker";
/// 取 marker 所在视觉行的 marker 范围的参数化属性名。
pub const LINE_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE: &str =
    "AXLineTextMarkerRangeForTextMarker";
/// 取 marker 所在行的上一行行首 marker 的参数化属性名。
pub const PREVIOUS_LINE_START_MARKER_PARAMETERIZED_ATTRIBUTE: &str =
    "AXPreviousLineStartTextMarkerForTextMarker";
/// 取 marker 所在行的下一行行尾 marker 的参数化属性名。
pub const NEXT_LINE_END_MARKER_PARAMETERIZED_ATTRIBUTE: &str =
    "AXNextLineEndTextMarkerForTextMarker";
/// 由两个 marker（数组，无需有序）构造范围的参数化属性名。
pub const RANGE_FOR_UNORDERED_MARKERS_PARAMETERIZED_ATTRIBUTE: &str =
    "AXTextMarkerRangeForUnorderedTextMarkers";
