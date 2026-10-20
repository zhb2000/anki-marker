//! macOS 取词实现：优先辅助功能 API（AXUIElement），失败回退 AppleScript 模拟 Cmd+C。
//!
//! 实现参考了 yetone/get-selected-text（MIT / Apache-2.0）的 macOS 部分：
//! 优先通过辅助功能 API（AXUIElement）直接读取选中文字；
//! 目标应用不兼容时回退为执行 AppleScript——临时静音系统提示音、模拟按下
//! Cmd+C 读取剪贴板后恢复剪贴板与音量。“不兼容”包括两种情况：AX 调用报错，
//! 以及 AX “成功”但返回空串（如 VSCode 的 Monaco 编辑器用 canvas 自绘文本、
//! 另挂隐藏 textarea 接收输入，AXSelectedText 恒为空，见
//! docs/selected-text-research.local.md）。
//! 两条路径均要求在“系统设置 → 隐私与安全性 → 辅助功能”中授权本应用。
//!
//! 取句的几类提供方：标准 AX 文本元素（AXSelectedTextRange + AXStringForRange）、
//! text marker 提供方（AXTextMarker，如 Safari 网页、Chromium 系，见 marker_selected_context）、
//! PDFKit 文档（Preview、
//! Safari 的 PDF 查看器：选区位置不可得，经整页文本匹配定位，见
//! find_sentence_in_pdf_pages）。
//!
//! 回退 Cmd+C 后“剪贴板未变”的语义按 AX 路径分流（借鉴 Windows 端）：AX 能正常
//! 响应（返回空串）= 没读到选区，返回空串由调用方提示“没有选区”；AX 查询本身报错
//! 且剪贴板不变 = 无法证明目标里有选区，报取词失败。每次取词都会记日志：取词目标
//! 的 pid/进程名（`capture target`，目标是本应用自己时会特别标明）与最终结果预览。
//!
//! 选词取句（word-to-sentence）开启时入口为 get_selected_context：
//! AX 先读选中的词，再经 AXSelectedTextRange / AXStringForRange 读取词所在的
//! 上下文窗口并切出整个句子；选区横跨多个句子时（用户手动跨句/跨行选择）
//! 原样录入所选文本——只扩展、不丢弃；取句链路任一环节失败时降级为仅录入
//! 词原文，与既有的 AX 成功路径语义一致。

use super::capture::{capture_in_context, describe_context, SentenceCapture};
use super::{RetryCallbacks, SelectedContext};

pub fn get_selected_text() -> Result<String, String> {
    match get_selected_text_by_ax() {
        Ok(text) if !text.trim().is_empty() => return Ok(text),
        Ok(_) => {
            // AX “成功”但为空：可能是真的没选中文本，也可能是目标应用不通过 AX
            // 暴露选区（如 VSCode），无法区分，统一回退到模拟 Cmd+C 再判一次；
            // AX 能正常响应，剪贴板不变即视为没选中——返回空串由调用方提示
            return get_selected_text_by_applescript(true);
        }
        Err(error) => {
            // AX 查询本身失败：剪贴板不变不能说明“没选中”（取词目标可能取错了、
            // 焦点在安全输入框、应用不理会模拟按键），按取词失败报错
            log::warn!("AX path failed, falling back to AppleScript: {error}");
            return get_selected_text_by_applescript(false);
        }
    }
}

/// word_to_sentence 为 true 时尝试“选词取句”：AX 全链路成功返回 {句子, 词}；
/// AX 拿到词但取句失败返回 {词原文, None}；AX 报错或词为空时回退 AppleScript（同现有逻辑）。
/// word_to_sentence 为 false 时完全等同现有 get_selected_text 的行为。
///
/// `callbacks`：AX 失败回退后，若目标应用的无障碍树在后台异步物化（如 Word），
/// 后台重试取到完整句子时经 `on_finished` 补发结果；`on_started`/`on_finished`
/// 同时供 UI 呈现“仍在补取”状态（见 RetryCallbacks 的调用约定）。
pub fn get_selected_context(
    word_to_sentence: bool,
    callbacks: RetryCallbacks,
) -> Result<SelectedContext, String> {
    if !word_to_sentence {
        let context = get_selected_text().map(|text| SelectedContext { text, word: None })?;
        log::info!("capture (sentence mode off): {}", describe_context(&context));
        return Ok(context);
    }
    // AX 报错（true）与 AX 正常但选区为空（false）在回退 Cmd+C 后对“剪贴板未变”
    // 的解读不同，见 get_selected_text_by_applescript 的 silent_if_unchanged
    // AX 报错时的补全目标：模拟复制只拿到词后，尝试用目标应用的 PDF 页面文本取句
    let mut pdf_refine_pid: Option<i32> = None;
    let silent_if_unchanged = match get_selected_context_by_ax() {
        Ok(context) if !context.text.trim().is_empty() => {
            log::info!("capture via AX: {}", describe_context(&context));
            return Ok(context);
        }
        Ok(_) => {
            // AX “成功”但为空：无法区分“真没选”与“应用不暴露选区”（如 VSCode 的
            // Monaco 编辑器），回退模拟 Cmd+C 再判一次；记日志便于诊断各应用的 AX 暴露情况
            log::warn!(
                "AX path returned empty selected text (no selection or the app does not \
                 expose its selection via AX), falling back to AppleScript"
            );
            true
        }
        Err(ax_error) => {
            // AX 失败（含树搜索无果）：立即回退 AppleScript 保证录入不阻塞；
            // 若拿到了目标应用 pid，同时启动后台重试——无障碍树异步物化后补发句子
            if let Some(pid) = ax_error.retry_pid {
                log::info!("spawning background AX selection retry for pid {pid}");
                spawn_selection_retry(pid, callbacks);
            }
            log::warn!("AX path failed, falling back to AppleScript: {}", ax_error.message);
            pdf_refine_pid = ax_error.retry_pid.or_else(frontmost_application_pid);
            false
        }
    };
    let mut context = get_selected_text_by_applescript(silent_if_unchanged)
        .map(|text| SelectedContext { text, word: None })?;
    if let Some(pid) = pdf_refine_pid {
        if !context.text.trim().is_empty() {
            if let Some(refined) = refine_with_pdf_pages(pid, &context.text) {
                context = refined;
            }
        }
    }
    log::info!("capture via simulated Cmd+C: {}", describe_context(&context));
    return Ok(context);
}

/// macOS 辅助功能 C API（ApplicationServices 框架）的最小 FFI 绑定。
///
/// 仅绑定读取选中文本与选词取句所需的符号，避免引入 `accessibility-ng`/`cocoa`
/// 整条依赖链（其中包含存在 future-incompat 问题的 `block` crate）。
///
/// AX 属性名不通过链接 C 全局符号获取（`kAX*Attribute` 等数据符号在
/// framework 的 tbd 导出中无法解析，会导致链接失败），而是直接使用其字符串值。
mod ax_ffi {
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
        /// 取 AXTextMarkerRange 的起点 marker（Copy 规则，返回值已 retain，调用者
        /// 负责释放）。AXTextMarker 体系公开 C API，随 HIServices 导出。
        pub fn AXTextMarkerRangeCopyStartMarker(range: CFTypeRef) -> CFTypeRef;
        /// 取 AXTextMarkerRange 的终点 marker（规则同上）。
        pub fn AXTextMarkerRangeCopyEndMarker(range: CFTypeRef) -> CFTypeRef;
        /// 创建包装 C 结构体的 AXValue（Create 规则，返回值已 retain，调用者负责释放）。
        pub fn AXValueCreate(
            the_type: AXValueType,
            value_ptr: *const std::ffi::c_void,
        ) -> CFTypeRef;
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
    pub const ATTRIBUTED_STRING_FOR_RANGE_PARAMETERIZED_ATTRIBUTE: &str =
        "AXAttributedStringForRange";
    /// “选区 marker 范围”属性名（AXTextMarker 体系：Safari 网页、Chromium 系等提供方
    /// 不提供 AXSelectedText / AXSelectedTextRange，选区只经此属性暴露）。
    pub const SELECTED_TEXT_MARKER_RANGE_ATTRIBUTE: &str = "AXSelectedTextMarkerRange";
    /// 按 marker 范围取子串的参数化属性名。
    pub const STRING_FOR_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE: &str =
        "AXStringForTextMarkerRange";
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
}

/// 获取系统当前焦点 UI 元素。
///
/// 注意：未授予辅助功能权限时此处会失败，由调用方回退或提示。
fn get_focused_ui_element_by_ax() -> Result<core_foundation::base::CFType, String> {
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::string::CFString;

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
        let focused: core_foundation::base::CFType = TCFType::wrap_under_create_rule(focused_ref);
        // 诊断：记录取词实际打到哪个应用上（“读错控件”“目标取成了本应用自己”
        // 一类问题全靠这条日志定位）
        log::info!("capture target: {}", describe_element_owner(&focused));
        return Ok(focused);
    }
}

/// 焦点元素所属进程的描述串（pid、进程名；是本应用自己时特别标明）。
fn describe_element_owner(element: &core_foundation::base::CFType) -> String {
    let Some(pid) = get_element_pid(element) else {
        return "unknown process".to_string();
    };
    let name = process_name(pid).unwrap_or_else(|| "?".to_string());
    return if pid as u32 == std::process::id() {
        format!("pid {pid} ({name}) — this app itself, not the target app")
    } else {
        format!("pid {pid} ({name})")
    };
}

/// 按 pid 查进程名（libproc 的 proc_name，随 libSystem 链接）；失败返回 None。
fn process_name(pid: i32) -> Option<String> {
    extern "C" {
        fn proc_name(pid: i32, buffer: *mut std::ffi::c_void, buffersize: u32) -> i32;
    }
    let mut buffer = [0u8; 128];
    let length = unsafe { proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 {
        return None;
    }
    return Some(String::from_utf8_lossy(&buffer[..length as usize]).into_owned());
}

/// 读取辅助功能元素的字符串属性值；AX 报错、空指针或值非字符串时返回 Err。
fn copy_string_attribute_by_ax(
    element: &core_foundation::base::CFType,
    attribute: &'static str,
) -> Result<String, String> {
    use core_foundation::base::TCFType;

    return copy_string_attribute_by_ax_ref(element.as_CFTypeRef(), attribute);
}

/// copy_string_attribute_by_ax 的 CFTypeRef 版本（供树搜索直接操作借用的元素引用）。
fn copy_string_attribute_by_ax_ref(
    element: core_foundation::base::CFTypeRef,
    attribute: &'static str,
) -> Result<String, String> {
    use core_foundation::base::{CFGetTypeID, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    unsafe {
        let attribute_ref = CFString::from_static_string(attribute);
        let mut value_ref: CFTypeRef = std::ptr::null();
        let err = ax_ffi::AXUIElementCopyAttributeValue(
            element,
            attribute_ref.as_concrete_TypeRef(),
            &mut value_ref,
        );
        if err != 0 || value_ref.is_null() {
            return Err(format!("failed to copy attribute \"{attribute}\": AXError {err}"));
        }
        if CFGetTypeID(value_ref) != CFString::type_id() {
            return Err(format!("value of attribute \"{attribute}\" is not a string"));
        }
        let value: CFString = TCFType::wrap_under_create_rule(value_ref as CFStringRef);
        return Ok(value.to_string());
    }
}

/// 读取辅助功能元素的引用型属性（如 AXWindow、AXFocusedWindow），返回未解析的 CFType。
fn copy_element_attribute_by_ax(
    element: &core_foundation::base::CFType,
    attribute: &'static str,
) -> Result<core_foundation::base::CFType, String> {
    use core_foundation::base::{CFTypeRef, TCFType};
    use core_foundation::string::CFString;

    unsafe {
        let attribute_ref = CFString::from_static_string(attribute);
        let mut value_ref: CFTypeRef = std::ptr::null();
        let err = ax_ffi::AXUIElementCopyAttributeValue(
            element.as_CFTypeRef(),
            attribute_ref.as_concrete_TypeRef(),
            &mut value_ref,
        );
        if err != 0 || value_ref.is_null() {
            return Err(format!("failed to copy attribute \"{attribute}\": AXError {err}"));
        }
        // AXUIElementCopyAttributeValue 遵循 Copy 规则（返回已 retain 的对象）。
        return Ok(TCFType::wrap_under_create_rule(value_ref));
    }
}

/// 读取辅助功能元素的子元素数组（AXChildren，Create 规则返回，随 Drop 释放）。
fn copy_children_array_by_ax(
    element: core_foundation::base::CFTypeRef,
) -> Result<core_foundation::base::CFType, String> {
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFGetTypeID, CFType, CFTypeRef, TCFType};
    use core_foundation::string::CFString;

    unsafe {
        let attribute = CFString::from_static_string(ax_ffi::CHILDREN_ATTRIBUTE);
        let mut value_ref: CFTypeRef = std::ptr::null();
        let err = ax_ffi::AXUIElementCopyAttributeValue(
            element,
            attribute.as_concrete_TypeRef(),
            &mut value_ref,
        );
        if err != 0 || value_ref.is_null() {
            return Err(format!(
                "failed to copy attribute \"{}\": AXError {err}",
                ax_ffi::CHILDREN_ATTRIBUTE
            ));
        }
        if CFGetTypeID(value_ref) != CFArray::<CFType>::type_id() {
            return Err(format!(
                "value of attribute \"{}\" is not an array",
                ax_ffi::CHILDREN_ATTRIBUTE
            ));
        }
        return Ok(TCFType::wrap_under_create_rule(value_ref));
    }
}

/// 读取辅助功能元素的整型属性值（如 AXNumberOfCharacters）；
/// AX 报错、空指针、值非数字或数字超出 i64 时返回 Err。
fn copy_i64_attribute_by_ax(
    element: &core_foundation::base::CFType,
    attribute: &'static str,
) -> Result<i64, String> {
    use core_foundation::base::{CFGetTypeID, CFTypeRef, TCFType};
    use core_foundation::number::{CFNumber, CFNumberRef};
    use core_foundation::string::CFString;

    unsafe {
        let attribute_ref = CFString::from_static_string(attribute);
        let mut value_ref: CFTypeRef = std::ptr::null();
        let err = ax_ffi::AXUIElementCopyAttributeValue(
            element.as_CFTypeRef(),
            attribute_ref.as_concrete_TypeRef(),
            &mut value_ref,
        );
        if err != 0 || value_ref.is_null() {
            return Err(format!("failed to copy attribute \"{attribute}\": AXError {err}"));
        }
        if CFGetTypeID(value_ref) != CFNumber::type_id() {
            return Err(format!("value of attribute \"{attribute}\" is not a number"));
        }
        let value: CFNumber = TCFType::wrap_under_create_rule(value_ref as CFNumberRef);
        return value
            .to_i64()
            .ok_or_else(|| format!("value of attribute \"{attribute}\" does not fit in i64"));
    }
}

/// 通过辅助功能 API 读取当前焦点元素中的选中文本。
///
/// 注意：未授予辅助功能权限时此处会失败，由调用方回退或提示。
fn get_selected_text_by_ax() -> Result<String, String> {
    let focused = get_focused_ui_element_by_ax()?;
    return match copy_string_attribute_by_ax(&focused, ax_ffi::SELECTED_TEXT_ATTRIBUTE) {
        Ok(text) => Ok(text),
        // text marker 提供方（Safari 网页等）不提供 AXSelectedText，改读 marker 范围
        Err(error) => match marker_selected_text(&focused) {
            Some(text) => Ok(text),
            None => Err(error),
        },
    };
}

/// AX 取句链路的失败：携带诊断信息与（可能存在的）后台重试句柄（目标应用 pid）。
#[derive(Debug)]
struct AxAttemptError {
    message: String,
    retry_pid: Option<i32>,
}

/// 通过辅助功能 API 读取选中的词及其所在的句子（选词取句的 AX 链路）。
///
/// 成功取句返回 {整句, Some(词)}；用户手动跨句选择时返回 {所选原文, None}；
/// 拿到词但取句链路任一环节失败时降级返回 {词原文, None}（语义等同现有的
/// AX 成功路径，不再回退 AppleScript）；连词都取不到时才返回 Err（携带
/// 后台重试句柄——部分应用的无障碍树在被查询后异步物化，如 Word）。
fn get_selected_context_by_ax() -> Result<SelectedContext, AxAttemptError> {
    let focused = get_focused_ui_element_by_ax().map_err(|error| AxAttemptError {
        message: error,
        retry_pid: None,
    })?;
    let word = match copy_string_attribute_by_ax(&focused, ax_ffi::SELECTED_TEXT_ATTRIBUTE) {
        Ok(word) => word,
        Err(focused_error) => {
            // text marker 提供方（Safari 网页、Chromium 系等）：选区只经 AXTextMarkerRange
            // 暴露，先走 marker 链路。命中即返回——含“确认是此类提供方但没有选区”的空结果，避免对没选中的
            // 常见操作白白做树搜索与后台重试
            if let Some(context) = marker_selected_context(&focused) {
                return Ok(context);
            }
            // 焦点元素读不到选区（如 Zotero 笔记编辑器：kAXErrorNoValue，焦点落在
            // 容器上）：立即做一次树搜索（部分应用首次即可命中）；无果则把目标
            // 应用 pid 交给后台重试——无障碍树可能异步物化，实测 Word 第二次划词
            // 才成功，后台轮询可在同一次按键内补上完整句子
            match find_element_with_selection_by_ax(&focused) {
                Ok(Some((element, word))) => {
                    log::info!(
                        "found the selection via AX tree search \
                         (focused element error was: {focused_error})"
                    );
                    return Ok(context_from_word_and_element(&element, &word));
                }
                _ => {
                    let retry_pid = get_element_pid(&focused);
                    return Err(AxAttemptError {
                        message: format!(
                            "{focused_error}; AX tree search found no element with a selection"
                        ),
                        retry_pid,
                    });
                }
            }
        }
    };
    if word.trim().is_empty() {
        // 与现有 get_selected_text 的空串语义一致：由调用方回退 AppleScript
        return Ok(SelectedContext { text: word, word: None });
    }
    return Ok(context_from_word_and_element(&focused, &word));
}

/// 参数化属性读取（Copy 规则，返回值随 Drop 释放）；AX 报错或空指针返回 Err。
fn copy_parameterized_attribute_by_ax(
    element: &core_foundation::base::CFType,
    attribute: &'static str,
    parameter: core_foundation::base::CFTypeRef,
) -> Result<core_foundation::base::CFType, String> {
    use core_foundation::base::{CFTypeRef, TCFType};
    use core_foundation::string::CFString;

    unsafe {
        let attribute_ref = CFString::from_static_string(attribute);
        let mut value_ref: CFTypeRef = std::ptr::null();
        let err = ax_ffi::AXUIElementCopyParameterizedAttributeValue(
            element.as_CFTypeRef(),
            attribute_ref.as_concrete_TypeRef(),
            parameter,
            &mut value_ref,
        );
        if err != 0 || value_ref.is_null() {
            return Err(format!("failed to copy parameterized attribute \"{attribute}\": AXError {err}"));
        }
        return Ok(TCFType::wrap_under_create_rule(value_ref));
    }
}

/// 读取 marker 范围对应的文本（AXStringForTextMarkerRange）。
fn string_for_marker_range_by_ax(
    element: &core_foundation::base::CFType,
    range: &core_foundation::base::CFType,
) -> Result<String, String> {
    use core_foundation::base::{CFGetTypeID, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    let value = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::STRING_FOR_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
        range.as_CFTypeRef(),
    )?;
    unsafe {
        if CFGetTypeID(value.as_CFTypeRef()) != CFString::type_id() {
            return Err("string for marker range is not a string".to_string());
        }
        let string: CFString = TCFType::wrap_under_get_rule(value.as_CFTypeRef() as CFStringRef);
        return Ok(string.to_string());
    }
}

/// text marker 提供方（Safari 网页及内嵌 WebKit 的应用、Chromium 系等）的选中文本。
///
/// 这类提供方不提供 AXSelectedText / AXSelectedTextRange，选区只经 AXTextMarker 体系
/// 暴露（实测 Safari：任一网页元素的 AXSelectedTextMarkerRange 都指向页面级选区，
/// 经 AXStringForTextMarkerRange 读文本）。元素没有该属性返回 None，空选区返回 Some("")。
fn marker_selected_text(element: &core_foundation::base::CFType) -> Option<String> {
    let range = copy_element_attribute_by_ax(element, ax_ffi::SELECTED_TEXT_MARKER_RANGE_ATTRIBUTE)
        .ok()?;
    return Some(string_for_marker_range_by_ax(element, &range).unwrap_or_default());
}

/// text marker 提供方的选词取句：选区 marker 范围 → 词；所在段落的 marker 范围 → 上下文；
/// “段首到选区起点”的前缀范围文本长度 → 选区在段内的 UTF-16 偏移 → 共享 capture
/// 校验锚点并切句（与 Windows UIA 的前缀法同构）。
///
/// 返回 None = 不是此类提供方（调用方走其他链路）；Some(空文本) = 是但没有选区；
/// 取句失败降级为仅录词原文。不用提供方自带的句子范围
/// （AXSentenceTextMarkerRangeForTextMarker），以保持各应用的分句规则一致。
fn marker_selected_context(
    element: &core_foundation::base::CFType,
) -> Option<SelectedContext> {
    let word = marker_selected_text(element)?;
    if word.trim().is_empty() {
        return Some(SelectedContext { text: word, word: None });
    }
    log::info!("found the selection via the text marker API");
    return Some(match find_sentence_for_marker_selection(element, &word) {
        Ok((SentenceCapture::Expanded(sentence), word)) => SelectedContext {
            text: strip_text_artifacts(&sentence),
            word: Some(strip_text_artifacts(word.trim())),
        },
        Ok((SentenceCapture::SelectionAsIs(text), _)) => {
            SelectedContext { text: strip_text_artifacts(&text), word: None }
        }
        Err(error) => {
            log::warn!("failed to extract sentence for the selected word: {error}");
            SelectedContext { text: strip_text_artifacts(&word), word: None }
        }
    });
}

/// 去掉提供方文本里不属于正文的字符：控制字符（Chromium 的 PDF 查看器把行尾连字符
/// 输出为 U+0002，如 `per\u{2}centage`）与软连字符 U+00AD。放在取句之后、只处理输出，
/// 不影响取句用的偏移。
fn strip_text_artifacts(text: &str) -> String {
    return text
        .chars()
        .filter(|c| *c != '\u{AD}' && !(c.is_control() && !c.is_whitespace()))
        .collect();
}

/// 句子是否看起来被上下文截断：以小写字母开头（句子不会以小写开头，说明左侧被截），
/// 或不以句末标点（含其后的引号、括号）结尾（右侧被截）。仅用于决定是否扩大上下文重试。
fn looks_truncated(sentence: &str) -> bool {
    let starts_lowercase = sentence.chars().next().is_some_and(char::is_lowercase);
    let trimmed = sentence.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '”' | '’' | ')' | ']' | '»' | '」' | '』' | '）')
    });
    let ends_terminal = trimmed
        .chars()
        .next_back()
        .is_some_and(|c| matches!(c, '.' | '!' | '?' | '…' | '。' | '！' | '？'));
    return starts_lowercase || !ends_terminal;
}

/// 把换行符逐个换成空格（长度不变，UTF-16 偏移不受影响）。
///
/// 视觉行硬换行（Chromium 的 PDF 查看器等把文本按视觉行给出，行间是 `\r\n`）会使
/// UAX #29 强制分句（SB4），把跨行整句切成碎片；扩大上下文取句时先抹掉换行。
fn flatten_line_breaks(text: &str) -> String {
    return text.chars().map(|c| if c == '\r' || c == '\n' { ' ' } else { c }).collect();
}

/// 折叠连续空白为单个空格（抹换行产生的双空格、行首行尾空白一并收拾）。
fn collapse_whitespace(text: &str) -> String {
    return text.split_whitespace().collect::<Vec<_>>().join(" ");
}

/// text marker 提供方的取句：先用选区起点所在“段落”作上下文；段落过短导致句子被截断、
/// 或段落装不下整个选区（Chromium 的 PDF 查看器里“段落”只是一个文本片段）时，
/// 改用行导航向前后扩展上下文重试。返回 {取句结果, 应上报的词}——词在抹换行的
/// 重试路径下与句子保持同一文本形态。
fn find_sentence_for_marker_selection(
    element: &core_foundation::base::CFType,
    word: &str,
) -> Result<(SentenceCapture, String), String> {
    let selection =
        copy_element_attribute_by_ax(element, ax_ffi::SELECTED_TEXT_MARKER_RANGE_ATTRIBUTE)?;
    return find_sentence_for_marker_range(element, word, &selection);
}

/// find_sentence_for_marker_selection 的取句部分：选区 marker 范围已知。
fn find_sentence_for_marker_range(
    element: &core_foundation::base::CFType,
    word: &str,
    selection: &core_foundation::base::CFType,
) -> Result<(SentenceCapture, String), String> {
    use core_foundation::base::{CFType, TCFType};

    /// 行导航扩展的行数梯度：每级向前后各扩这么多行；上一级仍被截断才升级。
    /// 一行约 50 到 70 个码元，10 行覆盖绝大多数句子，40 行兜长句。
    const LINE_EXPANSION_STEPS: [usize; 2] = [10, 40];

    let marker_of = |range: &CFType, end: bool| -> Result<CFType, String> {
        let marker = unsafe {
            if end {
                ax_ffi::AXTextMarkerRangeCopyEndMarker(range.as_CFTypeRef())
            } else {
                ax_ffi::AXTextMarkerRangeCopyStartMarker(range.as_CFTypeRef())
            }
        };
        if marker.is_null() {
            return Err("the marker range has no start/end marker".to_string());
        }
        return Ok(unsafe { TCFType::wrap_under_create_rule(marker) });
    };
    let selection_start = marker_of(selection, false)?;
    let selection_end = marker_of(selection, true)?;

    // 第一轮：选区起点所在段落（网页的段落本来就是整块文本，绝大多数情况止于此）
    let paragraph = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::PARAGRAPH_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
        selection_start.as_CFTypeRef(),
    )?;
    let paragraph_start = marker_of(&paragraph, false)?;
    let first = capture_in_marker_context(
        element,
        word,
        &paragraph_start,
        &paragraph,
        &selection_start,
        false,
    );
    // 扩展的触发条件：句子看起来被截断，且成因可能是上下文太短——句子贴着上下文边缘，
    // 或上下文里有硬换行（视觉行换行会让 UAX #29 在段落内部强制分句，句子不必触边）。
    // 网页里整块的段落既不含换行、句子也不贴边，不会进入第二轮
    match &first {
        Ok((SentenceCapture::Expanded(sentence), has_line_break, touched_edge))
            if looks_truncated(sentence) && (*touched_edge || *has_line_break) => {}
        Ok((capture, _, _)) => return Ok((clone_capture(capture), word.to_string())),
        Err(_) => {}
    }

    // 第二轮起：行导航扩展。锚点是选区所在的整行，向前后各走若干行
    let line_start_of = |marker: &CFType, end: bool| -> Result<CFType, String> {
        let line = copy_parameterized_attribute_by_ax(
            element,
            ax_ffi::LINE_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
            marker.as_CFTypeRef(),
        )?;
        return marker_of(&line, end);
    };
    let (anchor_left, anchor_right) = match (
        line_start_of(&selection_start, false),
        line_start_of(&selection_end, true),
    ) {
        (Ok(left), Ok(right)) => (left, right),
        (Err(error), _) | (_, Err(error)) => {
            return first
                .map(|(capture, _, _)| (capture, word.to_string()))
                .map_err(|first_error| format!("{first_error}; line navigation: {error}"));
        }
    };
    let flat_word = flatten_line_breaks(word);
    let mut best: Option<(SentenceCapture, String)> = None;
    for lines in LINE_EXPANSION_STEPS {
        let left = step_lines(
            element,
            ax_ffi::PREVIOUS_LINE_START_MARKER_PARAMETERIZED_ATTRIBUTE,
            &anchor_left,
            lines,
        );
        let right = step_lines(
            element,
            ax_ffi::NEXT_LINE_END_MARKER_PARAMETERIZED_ATTRIBUTE,
            &anchor_right,
            lines,
        );
        let markers = core_foundation::array::CFArray::from_CFTypes(&[left.clone(), right]);
        let Ok(range) = copy_parameterized_attribute_by_ax(
            element,
            ax_ffi::RANGE_FOR_UNORDERED_MARKERS_PARAMETERIZED_ATTRIBUTE,
            markers.as_CFTypeRef(),
        ) else {
            break;
        };
        match capture_in_marker_context(element, &flat_word, &left, &range, &selection_start, true)
        {
            Ok((capture, _, touched_edge)) => {
                let truncated = touched_edge
                    && matches!(&capture, SentenceCapture::Expanded(s) if looks_truncated(s));
                log::info!(
                    "expanded the context by {lines} line(s) on each side, sentence still \
                     looks truncated: {truncated}"
                );
                let capture = match capture {
                    SentenceCapture::Expanded(text) => {
                        SentenceCapture::Expanded(collapse_whitespace(&text))
                    }
                    SentenceCapture::SelectionAsIs(text) => {
                        SentenceCapture::SelectionAsIs(collapse_whitespace(&text))
                    }
                };
                best = Some((capture, collapse_whitespace(&flat_word)));
                if !truncated {
                    break;
                }
            }
            Err(error) => {
                log::info!("capture with the {lines}-line context failed: {error}");
            }
        }
    }
    return match (best, first) {
        (Some(result), _) => Ok(result),
        // 扩展没有任何进展：沿用第一轮结果（可能是被截断的句子，或锚点失败的错误）
        (None, Ok((capture, _, _))) => Ok((capture, word.to_string())),
        (None, Err(error)) => Err(error),
    };
}

fn clone_capture(capture: &SentenceCapture) -> SentenceCapture {
    return match capture {
        SentenceCapture::Expanded(text) => SentenceCapture::Expanded(text.clone()),
        SentenceCapture::SelectionAsIs(text) => SentenceCapture::SelectionAsIs(text.clone()),
    };
}

/// 从 `from` 起沿 `attribute`（上一行行首 / 下一行行尾）走至多 `steps` 步；
/// 到头（报错或 marker 不再变化）即停，返回最后一个有效 marker。
fn step_lines(
    element: &core_foundation::base::CFType,
    attribute: &'static str,
    from: &core_foundation::base::CFType,
    steps: usize,
) -> core_foundation::base::CFType {
    use core_foundation::base::TCFType;

    let mut current = from.clone();
    for _ in 0..steps {
        match copy_parameterized_attribute_by_ax(element, attribute, current.as_CFTypeRef()) {
            Ok(next) if next != current => current = next,
            _ => break,
        }
    }
    return current;
}

/// 在 `context_range` 对应的文本上取句：偏移 = “上下文起点到选区起点”的前缀长度。
/// `flatten` 为 true 时先抹掉上下文里的换行（`word` 由调用方同步处理）。
/// 返回 {取句结果, 上下文是否含换行, 句子是否触边}。
fn capture_in_marker_context(
    element: &core_foundation::base::CFType,
    word: &str,
    context_start: &core_foundation::base::CFType,
    context_range: &core_foundation::base::CFType,
    selection_start: &core_foundation::base::CFType,
    flatten: bool,
) -> Result<(SentenceCapture, bool, bool), String> {
    use core_foundation::array::CFArray;
    use core_foundation::base::TCFType;

    /// 上下文的 UTF-16 码元上限：防御病态页面（无块级分隔的巨型文本节点），
    /// 超限降级为仅录词，不为切一句话读取整页文本
    const MAX_CONTEXT_UTF16_UNITS: usize = 50_000;

    let raw_context = string_for_marker_range_by_ax(element, context_range)?;
    let context_units = raw_context.encode_utf16().count();
    if context_units > MAX_CONTEXT_UTF16_UNITS {
        return Err(format!(
            "the context is too long ({context_units} utf16 units), skipping sentence extraction"
        ));
    }
    let markers = CFArray::from_CFTypes(&[context_start.clone(), selection_start.clone()]);
    let prefix = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::RANGE_FOR_UNORDERED_MARKERS_PARAMETERIZED_ATTRIBUTE,
        markers.as_CFTypeRef(),
    )?;
    let sel_start_utf16 = string_for_marker_range_by_ax(element, &prefix)?
        .encode_utf16()
        .count() as isize;
    let has_line_break = raw_context.contains('\n');
    let context = if flatten { flatten_line_breaks(&raw_context) } else { raw_context };

    // 偏移是本地推导的，与 Windows 同理不用“唯一出现位置”兜底（见 capture::find_anchor）。
    // window_loc 传 1：上下文起点不是文本开头，句子贴着起点同样算触边
    return match capture_in_context(&context, word, &[sel_start_utf16], 1, false) {
        Some((capture, touched_edge)) => Ok((capture, has_line_break, touched_edge)),
        None => Err(format!(
            "no validated anchor for the selected text in the context \
             (selection starts at {sel_start_utf16} utf16 units, context {context_units} utf16 \
             units, word {} utf16 units, {} occurrence(s) in the context; \
             a selection spanning several paragraphs lands here too)",
            word.encode_utf16().count(),
            context.match_indices(word).count()
        )),
    };
}

/// 读取元素所属应用的进程 id（AXUIElementGetPid），失败返回 None。
fn get_element_pid(element: &core_foundation::base::CFType) -> Option<i32> {
    use core_foundation::base::TCFType;

    let mut pid: i32 = 0;
    let err = unsafe { ax_ffi::AXUIElementGetPid(element.as_CFTypeRef(), &mut pid) };
    return if err == 0 && pid > 0 { Some(pid) } else { None };
}

/// 拿到所选文本后的收尾：对持有选区的元素跑取句链路，失败降级为仅录词原文。
fn context_from_word_and_element(
    element: &core_foundation::base::CFType,
    word: &str,
) -> SelectedContext {
    let context = match find_sentence_for_word_by_ax(element, word) {
        Ok(SentenceCapture::Expanded(sentence)) => {
            // 范围链路的上下文可能带视觉行硬换行（Chrome 的 PDF 查看器：同时提供
            // AXSelectedText 与 marker 范围，但 AXStringForRange 的文本按行给出），
            // UAX #29 在换行处强制分句，句子被切成碎片。看起来被截断、且元素支持
            // marker 范围时，改用 marker 链路（带行扩展与抹换行）重试
            if looks_truncated(&sentence) {
                if let Some(refined) = refine_truncated_with_markers(element, word) {
                    return refined;
                }
            }
            SelectedContext { text: sentence, word: Some(word.trim().to_string()) }
        }
        Ok(SentenceCapture::SelectionAsIs(text)) => SelectedContext { text, word: None },
        Err(error) => {
            // 范围链路失败。PDFKit（Preview、Safari 的 PDF 查看器）不暴露选区位置，
            // 尝试用整页文本匹配选中词来定位；仍失败则降级为现有 AX 成功路径的语义
            // （录入词原文）——已拿到词，取句失败不致命
            match find_sentence_in_pdf_pages(element, word) {
                Ok(context) => context,
                Err(pdf_error) => {
                    log::warn!(
                        "failed to extract sentence for the selected word: {error} \
                         (PDF page lookup: {pdf_error})"
                    );
                    SelectedContext { text: word.to_string(), word: None }
                }
            }
        }
    };
    return SelectedContext {
        text: strip_text_artifacts(&context.text),
        word: context.word.map(|word| strip_text_artifacts(&word)),
    };
}

/// 范围链路切出的句子被截断时，用 marker 链路重试：元素没有 marker 范围、或重试仍没有
/// 得到一个完整的句子，都返回 None（沿用范围链路的结果）。
fn refine_truncated_with_markers(
    element: &core_foundation::base::CFType,
    word: &str,
) -> Option<SelectedContext> {
    copy_element_attribute_by_ax(element, ax_ffi::SELECTED_TEXT_MARKER_RANGE_ATTRIBUTE).ok()?;
    return match find_sentence_for_marker_selection(element, word) {
        Ok((SentenceCapture::Expanded(sentence), word)) if !looks_truncated(&sentence) => {
            log::info!("the range chain's sentence looked truncated, refined via the text marker API");
            Some(SelectedContext {
                text: strip_text_artifacts(&sentence),
                word: Some(strip_text_artifacts(word.trim())),
            })
        }
        _ => None,
    };
}

/// 读取当前焦点应用的焦点窗口（AX 树搜索的备选根）。
fn get_focused_window_by_ax() -> Result<core_foundation::base::CFType, String> {
    use core_foundation::base::{CFType, TCFType};

    unsafe {
        let system_wide: CFType =
            TCFType::wrap_under_create_rule(ax_ffi::AXUIElementCreateSystemWide());
        let app = copy_element_attribute_by_ax(&system_wide, ax_ffi::FOCUSED_APPLICATION_ATTRIBUTE)?;
        return copy_element_attribute_by_ax(&app, ax_ffi::FOCUSED_WINDOW_ATTRIBUTE);
    }
}

/// 唤醒目标应用的无障碍支持（尽力而为）：Electron/Chromium 系应用监听
/// AXManualAccessibility 属性被设置来激活无障碍树；AXEnhancedUserInterface
/// 是另一类应用使用的开关。二者都设、失败忽略——实测 Word/Zotero 均不响应
///（-25205 / -25208），保留此机制给其他可能响应的应用一个机会。
fn wake_app_accessibility(app: &core_foundation::base::CFType) {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::string::CFString;

    for attribute in [
        ax_ffi::MANUAL_ACCESSIBILITY_ATTRIBUTE,
        ax_ffi::ENHANCED_USER_INTERFACE_ATTRIBUTE,
    ] {
        unsafe {
            let attribute_ref = CFString::from_static_string(attribute);
            let err = ax_ffi::AXUIElementSetAttributeValue(
                app.as_CFTypeRef(),
                attribute_ref.as_concrete_TypeRef(),
                CFBoolean::true_value().as_CFTypeRef(),
            );
            if err != 0 {
                log::info!("setting \"{attribute}\" on the application failed: AXError {err}");
            }
        }
    }
}

/// 在焦点元素所在窗口的 AX 子树中搜索持有非空选区的元素（入口：推导搜索根）。
fn find_element_with_selection_by_ax(
    focused: &core_foundation::base::CFType,
) -> Result<Option<(core_foundation::base::CFType, String)>, String> {
    // 搜索根：焦点元素所在的窗口；取不到时退到焦点应用的焦点窗口
    let window = copy_element_attribute_by_ax(focused, ax_ffi::WINDOW_ATTRIBUTE)
        .or_else(|_| get_focused_window_by_ax())?;
    return Ok(find_element_with_selection_in_window(&window));
}

/// 在给定窗口的 AX 子树中做有界 DFS（深度 ≤ 30、节点 ≤ 500，每次属性读取都是
/// 一次跨进程调用），找第一个 AXSelectedText 非空的元素；无果时记 info 级树
/// 形态统计日志——用于诊断提供方是否暴露了真实内容树（桩树访问节点很少，
/// 内容树节点多但可能大量元素的 AXSelectedText 为空或报错）。
fn find_element_with_selection_in_window(
    window: &core_foundation::base::CFType,
) -> Option<(core_foundation::base::CFType, String)> {
    use core_foundation::base::{CFType, CFTypeRef, TCFType};

    /// 搜索预算：深度与节点数上限（每次属性读取都是一次跨进程调用）
    const MAX_DEPTH: usize = 30;
    const MAX_NODES: usize = 500;

    /// DFS 统计（诊断提供方的树形态：桩树访问节点很少；内容树则节点多、
    /// 且可能大量元素报 AXSelectedText 为空或错误）
    #[derive(Default)]
    struct TreeSearchStats {
        visited: usize,
        max_depth: usize,
        empty_selected_count: usize,
    }

    fn dfs(
        element: CFTypeRef,
        depth: usize,
        budget: &mut usize,
        stats: &mut TreeSearchStats,
    ) -> Option<(CFType, String)> {
        if depth > MAX_DEPTH || *budget == 0 {
            return None;
        }
        *budget -= 1;
        stats.visited += 1;
        stats.max_depth = stats.max_depth.max(depth);
        // 自身是否持有选区
        if let Ok(text) = copy_string_attribute_by_ax_ref(element, ax_ffi::SELECTED_TEXT_ATTRIBUTE) {
            if !text.trim().is_empty() {
                // wrap_under_get_rule：retain 借用来的元素引用，供数组释放后使用
                let owned: CFType = unsafe { TCFType::wrap_under_get_rule(element) };
                return Some((owned, text));
            }
            stats.empty_selected_count += 1;
        }
        // 子元素数组的生命周期覆盖整个循环，其子元素引用为借用，无需单独释放
        let children = copy_children_array_by_ax(element).ok()?;
        let count = unsafe { ax_ffi::CFArrayGetCount(children.as_CFTypeRef()) };
        for index in 0..count {
            if *budget == 0 {
                return None;
            }
            let child = unsafe { ax_ffi::CFArrayGetValueAtIndex(children.as_CFTypeRef(), index) };
            if child.is_null() {
                continue;
            }
            if let Some(found) = dfs(child, depth + 1, budget, stats) {
                return Some(found);
            }
        }
        return None;
    }

    let mut budget = MAX_NODES;
    let mut stats = TreeSearchStats::default();
    let found = dfs(window.as_CFTypeRef(), 0, &mut budget, &mut stats);
    if found.is_none() {
        log::info!(
            "AX tree search finished without a match \
             (visited {}, max depth {}, budget exhausted: {}, \
             nodes with empty AXSelectedText: {})",
            stats.visited,
            stats.max_depth,
            budget == 0,
            stats.empty_selected_count
        );
    }
    return found;
}

/// 前台应用的 pid（NSWorkspace.frontmostApplication，不经过 AX）；本应用自己返回 None。
///
/// 全局快捷键不改变前台应用，故这就是取词目标。Safari 的 PDF 查看器等场景下 AX 的
/// 系统级焦点（AXFocusedUIElement / AXFocusedApplication）都报 NoValue，只能靠它。
fn frontmost_application_pid() -> Option<i32> {
    let pid = objc2::rc::autoreleasepool(|_| unsafe {
        let workspace: *mut objc2::runtime::AnyObject =
            objc2::msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let app: *mut objc2::runtime::AnyObject = objc2::msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: i32 = objc2::msg_send![app, processIdentifier];
        return Some(pid);
    })?;
    return (pid > 0 && pid as u32 != std::process::id()).then_some(pid);
}

/// 在窗口 AX 子树里找 PDFKit 页面（有界 DFS：深度、节点数都限制；Safari 的页面在
/// 深度 6 左右）。找到第一个 AXPage 即返回其全部兄弟页面。
fn find_pdf_pages_in_window(
    window: &core_foundation::base::CFType,
) -> Option<Vec<core_foundation::base::CFType>> {
    const MAX_DEPTH: usize = 10;
    const MAX_NODES: usize = 400;

    fn dfs(
        element: &core_foundation::base::CFType,
        depth: usize,
        budget: &mut usize,
    ) -> Option<Vec<core_foundation::base::CFType>> {
        if depth > MAX_DEPTH || *budget == 0 {
            return None;
        }
        *budget -= 1;
        let children = children_of(element);
        let pages: Vec<_> = children.iter().filter(|c| is_ax_page(c)).cloned().collect();
        if !pages.is_empty() {
            return Some(pages);
        }
        for child in &children {
            if let Some(found) = dfs(child, depth + 1, budget) {
                return Some(found);
            }
        }
        return None;
    }
    let mut budget = MAX_NODES;
    return dfs(window, 0, &mut budget);
}

/// 模拟复制只拿到了词（AX 完全不暴露选区）时，用目标应用窗口里的 PDF 页面文本补全句子。
///
/// Safari 的 PDF 查看器就是这种：连焦点元素都取不到，页面元素也不报 AXSelectedText，
/// 选区只能靠 Cmd+C 读到；但它的页面元素仍提供整页文本，词可据此定位（见
/// find_sentence_in_pdf_pages）。目标不是 PDF 或定位不唯一时返回 None，保持仅录词。
fn refine_with_pdf_pages(pid: i32, selection: &str) -> Option<SelectedContext> {
    use core_foundation::base::{CFType, TCFType};

    let started = std::time::Instant::now();
    let app: CFType =
        unsafe { TCFType::wrap_under_create_rule(ax_ffi::AXUIElementCreateApplication(pid)) };
    let window = copy_element_attribute_by_ax(&app, ax_ffi::FOCUSED_WINDOW_ATTRIBUTE).ok()?;
    let Some(pages) = find_pdf_pages_in_window(&window) else {
        log::info!(
            "no PDF pages in the front window of pid {pid} ({:?}), keeping the word only",
            started.elapsed()
        );
        return None;
    };
    return match locate_sentence_in_pdf_pages(&pages, selection) {
        Ok(context) => {
            log::info!("PDF page text lookup succeeded ({:?})", started.elapsed());
            Some(context)
        }
        Err(error) => {
            log::warn!("PDF page text lookup failed: {error}");
            None
        }
    };
}

/// 屏幕坐标下的矩形（与 CGRect 内存布局一致，供 AXValueGetValue 解包 AXFrame）。
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
struct AxRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl AxRect {
    /// 与另一矩形的相交面积是否为正（仅相接不算）。
    fn intersects(&self, other: &AxRect) -> bool {
        let overlap_width = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
        let overlap_height =
            (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);
        return overlap_width > 0.0 && overlap_height > 0.0;
    }
}

/// 读取元素的屏幕外框（AXFrame）。
fn element_frame_by_ax(element: &core_foundation::base::CFType) -> Option<AxRect> {
    use core_foundation::base::TCFType;

    let value = copy_element_attribute_by_ax(element, ax_ffi::FRAME_ATTRIBUTE).ok()?;
    let mut rect = AxRect::default();
    let decoded = unsafe {
        ax_ffi::AXValueGetValue(
            value.as_CFTypeRef(),
            ax_ffi::AX_VALUE_CG_RECT_TYPE,
            &mut rect as *mut AxRect as *mut std::ffi::c_void,
        )
    };
    return (decoded != 0).then_some(rect);
}

/// 元素的子元素列表（各自 retain，不受数组生命周期约束）；读取失败返回空。
fn children_of(element: &core_foundation::base::CFType) -> Vec<core_foundation::base::CFType> {
    use core_foundation::base::{CFType, TCFType};

    let Ok(children) = copy_children_array_by_ax(element.as_CFTypeRef()) else {
        return Vec::new();
    };
    let count = unsafe { ax_ffi::CFArrayGetCount(children.as_CFTypeRef()) };
    let mut result = Vec::new();
    for index in 0..count {
        let raw = unsafe { ax_ffi::CFArrayGetValueAtIndex(children.as_CFTypeRef(), index) };
        if !raw.is_null() {
            let child: CFType = unsafe { TCFType::wrap_under_get_rule(raw) };
            result.push(child);
        }
    }
    return result;
}

fn is_ax_page(element: &core_foundation::base::CFType) -> bool {
    return copy_string_attribute_by_ax(element, ax_ffi::ROLE_ATTRIBUTE)
        .is_ok_and(|role| role == "AXPage");
}

/// 找出 PDFKit 文档的全部页面元素（AXPage 兄弟节点）。
///
/// 入口元素可能是页面本身或其后代（Safari：窗口树搜索命中的页面元素）→ 沿 AXParent
/// 上溯找到 AXPage，取其父节点下的全部 AXPage；也可能是页面的容器（Preview：焦点元素
/// 是 AXSharedDocumentContainer）→ 向下做有界广度搜索。找不到返回 None（不是 PDFKit 文档）。
fn find_pdf_pages(
    element: &core_foundation::base::CFType,
) -> Option<Vec<core_foundation::base::CFType>> {
    let pages_of = |parent: &core_foundation::base::CFType| -> Vec<core_foundation::base::CFType> {
        children_of(parent).into_iter().filter(is_ax_page).collect()
    };

    // 上溯：自身或祖先是 AXPage
    let mut current = element.clone();
    for _ in 0..12 {
        if is_ax_page(&current) {
            let parent = copy_element_attribute_by_ax(&current, ax_ffi::PARENT_ATTRIBUTE).ok()?;
            let pages = pages_of(&parent);
            return (!pages.is_empty()).then_some(pages);
        }
        match copy_element_attribute_by_ax(&current, ax_ffi::PARENT_ATTRIBUTE) {
            Ok(parent) => current = parent,
            Err(_) => break,
        }
    }

    // 下探：有界广度优先（层数、节点数都很小，PDF 文档容器紧挨着页面）
    const MAX_DEPTH: usize = 3;
    const MAX_NODES: usize = 80;
    let mut level = vec![element.clone()];
    let mut visited = 0;
    for _ in 0..MAX_DEPTH {
        let mut next_level = Vec::new();
        for node in &level {
            visited += 1;
            if visited > MAX_NODES {
                return None;
            }
            let children = children_of(node);
            let pages: Vec<_> = children.iter().filter(|c| is_ax_page(c)).cloned().collect();
            if !pages.is_empty() {
                return Some(pages);
            }
            next_level.extend(children);
        }
        level = next_level;
    }
    return None;
}

/// PDF 页面的可视区域：页面容器的最近 AXScrollArea 祖先（Preview），没有则取容器
/// 自身的外框（Safari 的 PDF 插件容器恰好就是视口）。
fn pdf_viewport(first_page: &core_foundation::base::CFType) -> Option<AxRect> {
    let container = copy_element_attribute_by_ax(first_page, ax_ffi::PARENT_ATTRIBUTE).ok()?;
    let mut current = container.clone();
    for _ in 0..8 {
        if copy_string_attribute_by_ax(&current, ax_ffi::ROLE_ATTRIBUTE)
            .is_ok_and(|role| role == "AXScrollArea")
        {
            return element_frame_by_ax(&current);
        }
        match copy_element_attribute_by_ax(&current, ax_ffi::PARENT_ATTRIBUTE) {
            Ok(parent) => current = parent,
            Err(_) => break,
        }
    }
    return element_frame_by_ax(&container);
}

/// 读取 PDF 页面的整页文本（UTF-16 码元序列）：AXAttributedStringForRange 取 [0, 字符数)。
fn pdf_page_text_utf16(page: &core_foundation::base::CFType) -> Result<Vec<u16>, String> {
    use core_foundation::base::{CFGetTypeID, CFRange, CFType, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    let count = copy_i64_attribute_by_ax(page, ax_ffi::NUMBER_OF_CHARACTERS_ATTRIBUTE)?;
    if count <= 0 {
        return Err("the page has no text".to_string());
    }
    let range = CFRange { location: 0, length: count as isize };
    unsafe {
        let range_value_ref = ax_ffi::AXValueCreate(
            ax_ffi::AX_VALUE_CF_RANGE_TYPE,
            &range as *const CFRange as *const std::ffi::c_void,
        );
        if range_value_ref.is_null() {
            return Err("failed to create AXValue for CFRange".to_string());
        }
        let range_value: CFType = TCFType::wrap_under_create_rule(range_value_ref);
        let attributed = copy_parameterized_attribute_by_ax(
            page,
            ax_ffi::ATTRIBUTED_STRING_FOR_RANGE_PARAMETERIZED_ATTRIBUTE,
            range_value.as_CFTypeRef(),
        )?;
        if CFGetTypeID(attributed.as_CFTypeRef()) != ax_ffi::CFAttributedStringGetTypeID() {
            return Err("the page text is not an attributed string".to_string());
        }
        // Get 规则：借用自 attributed，转成 owned CFString 后再用
        let string_ref = ax_ffi::CFAttributedStringGetString(attributed.as_CFTypeRef());
        if string_ref.is_null() {
            return Err("the attributed string has no string".to_string());
        }
        let string: CFString = TCFType::wrap_under_get_rule(string_ref as CFStringRef);
        return Ok(string.to_string().encode_utf16().collect());
    }
}

/// 在页面文本中找选中文本的全部出现位置（UTF-16 起止偏移），比较时忽略空白差异
/// （选区文本与页面文本在换行/空格上可能不同）。区分大小写；子串匹配（选区可能是
/// 半个词）——出现位置越多越容易判为歧义，这是保守方向。
fn find_selection_occurrences(page: &[u16], selection: &str) -> Vec<(usize, usize)> {
    let is_space = |unit: u16| char::from_u32(unit as u32).is_some_and(char::is_whitespace);
    let needle: Vec<u16> =
        selection.encode_utf16().filter(|unit| !is_space(*unit)).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    // 页面文本去空白后的序列及其到原偏移的映射
    let mut compact = Vec::with_capacity(page.len());
    let mut origin = Vec::with_capacity(page.len());
    for (index, unit) in page.iter().enumerate() {
        if !is_space(*unit) {
            compact.push(*unit);
            origin.push(index);
        }
    }
    let mut result = Vec::new();
    if compact.len() >= needle.len() {
        for start in 0..=(compact.len() - needle.len()) {
            if compact[start..start + needle.len()] == needle[..] {
                result.push((origin[start], origin[start + needle.len() - 1] + 1));
            }
        }
    }
    return result;
}

/// 以页面文本中的一处出现位置为锚点取句，返回 {句子/所选原文, 页面中的所选词}。
/// 只取出现位置前后各 CONTEXT_MARGIN 个码元的窗口，防止超长页面拖慢分句。
fn capture_pdf_occurrence(
    page: &[u16],
    start: usize,
    end: usize,
) -> Option<SelectedContext> {
    const CONTEXT_MARGIN: usize = 4096;
    let window_start = start.saturating_sub(CONTEXT_MARGIN);
    let window_end = (end + CONTEXT_MARGIN).min(page.len());
    let context = String::from_utf16_lossy(&page[window_start..window_end]);
    let word = String::from_utf16_lossy(&page[start..end]);
    // 偏移来自本地匹配，不用“唯一出现位置”兜底（见 capture::find_anchor）
    let (capture, _touched_edge) = capture_in_context(
        &context,
        &word,
        &[(start - window_start) as isize],
        window_start as isize,
        false,
    )?;
    return Some(match capture {
        SentenceCapture::Expanded(sentence) => {
            SelectedContext { text: sentence, word: Some(word.trim().to_string()) }
        }
        SentenceCapture::SelectionAsIs(text) => SelectedContext { text, word: None },
    });
}

/// PDFKit 文档（Preview、Safari 的 PDF 查看器）的选词取句。
///
/// PDFKit 的无障碍实现只给文档级的 AXSelectedText，不暴露选区位置（所有元素的
/// AXSelectedTextRange 都是占位值，AXBoundsForRange 也不可用）；但每个 AXPage 能经
/// AXAttributedStringForRange 给出整页文本。因此用“选中文本在页面文本中的出现位置”
/// 反推选区位置，按“宁可降级也不切错句”的原则只在能唯一确定时才取句：
/// 1. 先只看屏幕上可见的页面（选区必然在用户眼前）：恰好一处出现 → 定位成功；
/// 2. 可见页面里没有（视口计算失败，或选区已被滚出屏幕）→ 在其余页面（有上限）里找，
///    同样要求恰好一处；
/// 3. 多处出现：若各处切出的句子完全相同也接受，否则判歧义——高频词会降级为仅录词。
///
/// 已知的残余风险：选区被滚出屏幕、而可见页面里恰好有同一个词的唯一出现——会定位到
/// 可见页面里的那一处。
fn find_sentence_in_pdf_pages(
    element: &core_foundation::base::CFType,
    word: &str,
) -> Result<SelectedContext, String> {
    let pages = find_pdf_pages(element).ok_or("not a PDFKit document (no AXPage found)")?;
    return locate_sentence_in_pdf_pages(&pages, word);
}

/// find_sentence_in_pdf_pages 的定位部分：已知页面元素列表，按选中文本定位并取句。
fn locate_sentence_in_pdf_pages(
    pages: &[core_foundation::base::CFType],
    word: &str,
) -> Result<SelectedContext, String> {
    /// 第二阶段最多扫描的不可见页数（每页一次跨进程调用，页面文本可达上万字符）
    const MAX_HIDDEN_PAGES_SCANNED: usize = 40;

    let viewport = pdf_viewport(&pages[0]);
    let visible: Vec<bool> = pages
        .iter()
        .map(|page| match (&viewport, element_frame_by_ax(page)) {
            (Some(viewport), Some(frame)) => viewport.intersects(&frame),
            _ => false,
        })
        .collect();

    // 各页文本按需读取一次；candidates 为 (页下标, 起, 止)
    let mut texts: Vec<Option<Vec<u16>>> = vec![None; pages.len()];
    let mut candidates: Vec<(usize, usize, usize)> = Vec::new();
    let scan = |index: usize,
                    texts: &mut Vec<Option<Vec<u16>>>,
                    candidates: &mut Vec<(usize, usize, usize)>| {
        match pdf_page_text_utf16(&pages[index]) {
            Ok(text) => {
                for (start, end) in find_selection_occurrences(&text, word) {
                    candidates.push((index, start, end));
                }
                texts[index] = Some(text);
            }
            Err(error) => log::info!("PDF page {index}: cannot read the text: {error}"),
        }
    };

    let visible_count = visible.iter().filter(|v| **v).count();
    for index in (0..pages.len()).filter(|i| visible[*i]) {
        scan(index, &mut texts, &mut candidates);
    }
    let mut stage = "visible pages";
    if candidates.is_empty() {
        stage = "other pages";
        for index in (0..pages.len()).filter(|i| !visible[*i]).take(MAX_HIDDEN_PAGES_SCANNED) {
            scan(index, &mut texts, &mut candidates);
        }
    }
    log::info!(
        "PDF sentence capture: {} page(s), {visible_count} visible, {} occurrence(s) of the \
         selection found in the {stage}",
        pages.len(),
        candidates.len()
    );

    if candidates.is_empty() {
        return Err(format!(
            "the selected text ({} utf16 units) was not found in the page text",
            word.encode_utf16().count()
        ));
    }
    let mut contexts = Vec::new();
    for (index, start, end) in &candidates {
        let text = texts[*index].as_ref().expect("scanned page has text");
        contexts.push(
            capture_pdf_occurrence(text, *start, *end)
                .ok_or("failed to cut a sentence at the located occurrence")?,
        );
    }
    let first = contexts.remove(0);
    if contexts.iter().any(|other| other.text != first.text || other.word != first.word) {
        return Err(format!(
            "the selected text occurs {} times with different surrounding sentences, \
             the selection cannot be located",
            candidates.len()
        ));
    }
    return Ok(first);
}

/// 后台重试的互斥标记：0 表示无重试在途，否则为正在重试的应用 pid。
static RETRYING_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// 后台重试：部分应用（实测 Word）的无障碍树在首次被查询后才异步物化——
/// 首次划词时是只有百余节点的桩树，反复查询几秒后树才建成（所以第二次划词
/// 才成功）。此函数在后台按固定间隔轮询目标应用（pid 不随前台切换失效）：
/// 唤醒属性 → 树搜索 → 取句链路，树就绪后取到句子并通过 `on_finished` 补发。
/// 总轮询窗口约 RETRY_ATTEMPTS * RETRY_INTERVAL_MS，超时放弃（`on_finished(None)`）。
fn spawn_selection_retry(pid: i32, callbacks: RetryCallbacks) {
    use core_foundation::base::{CFType, TCFType};
    use std::sync::atomic::Ordering;

    /// 轮询次数与间隔（总计约 3 秒，覆盖实测的树物化耗时）
    const RETRY_ATTEMPTS: usize = 5;
    const RETRY_INTERVAL_MS: u64 = 600;

    // 同一时间只允许一个重试在途，避免连按快捷键叠加多个轮询线程
    // （被拦下时不触发任何回调：状态与在途的那个重试保持一致）
    if RETRYING_PID.swap(pid, Ordering::SeqCst) != 0 {
        return;
    }
    std::thread::spawn(move || {
        let RetryCallbacks { on_started, on_finished } = callbacks;
        on_started();
        for attempt in 1..=RETRY_ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(RETRY_INTERVAL_MS));
            unsafe {
                let app_ref = ax_ffi::AXUIElementCreateApplication(pid);
                if app_ref.is_null() {
                    continue;
                }
                let app: CFType = TCFType::wrap_under_create_rule(app_ref);
                wake_app_accessibility(&app);
                let window = match copy_element_attribute_by_ax(
                    &app,
                    ax_ffi::FOCUSED_WINDOW_ATTRIBUTE,
                ) {
                    Ok(window) => window,
                    Err(_) => continue,
                };
                if let Some((element, word)) = find_element_with_selection_in_window(&window) {
                    let context = context_from_word_and_element(&element, &word);
                    log::info!(
                        "AX selection retry succeeded on attempt {attempt}: {}",
                        describe_context(&context)
                    );
                    RETRYING_PID.store(0, Ordering::SeqCst);
                    on_finished(Some(context));
                    return;
                }
            }
        }
        RETRYING_PID.store(0, Ordering::SeqCst);
        on_finished(None);
    });
}

/// 取句主链路：读选区 CFRange → 窗口化读上下文 → 跨句判断 → 分句 → 触边重试 → 校验。
/// 任一环节失败返回 Err，由调用方降级为“仅录入词原文”。
fn find_sentence_for_word_by_ax(
    focused: &core_foundation::base::CFType,
    word: &str,
) -> Result<SentenceCapture, String> {
    use core_foundation::base::CFRange;

    let range = get_selected_text_range_by_ax(focused)?;

    // 文档字符数（UTF-16 码元计）：用于夹取窗口右缘。AppKit 等提供方对越界的
    // AXStringForRange 直接报错（不像 WebKit 宽容截断），不夹会导致短文档或
    // 选区靠近文档末尾时取句必然失败；取不到字符数时不夹，依赖提供方宽容
    // 截断或下方 fetch_context 的收缩重试。
    // 注意：字符数必须与选区自洽才可信——连选区都装不下的 count 与 range 不在
    // 同一坐标系（如 Chrome AXWebArea/PDF 返回块级或可见区计数），用于夹取
    // 会把窗口截断到选区之外，直接弃用（filter 条件）
    let char_count: Option<isize> =
        copy_i64_attribute_by_ax(focused, ax_ffi::NUMBER_OF_CHARACTERS_ATTRIBUTE)
            .ok()
            .map(|count| count as isize)
            .filter(|count| *count >= range.location + range.length);

    // 构造上下文窗口：左缘不越过文本开头，右缘不越过文档末尾（字符数已知且自洽时）
    let make_window = |left: isize, right: isize| -> CFRange {
        let loc = std::cmp::max(0, range.location - left);
        let mut length = range.length + (range.location - loc) + right;
        if let Some(count) = char_count {
            length = std::cmp::min(length, std::cmp::max(0, count - loc));
        }
        CFRange { location: loc, length }
    };

    // 抓取上下文：左缘 margin 固定，右缘从 margin 起逐级（÷4）收缩直到成功——
    // 有的提供方严格校验 range 右缘（越界即报错而非宽容截断），一步收缩到选区
    // 末尾会把可行右缘全部跳过，逐级收缩才能命中其文本的真实末尾（如块级/行级
    // 边界）。min_right 为此前已成功的右缘，保证触边重试只增不减。
    // 返回 (上下文, 实际成功的右缘)。
    let fetch_context = |margin: isize, min_right: isize| -> Result<(String, isize), String> {
        let mut right = margin;
        let mut last_error = "no window fetch attempted".to_string();
        while right > min_right {
            match copy_string_for_range_by_ax(focused, make_window(margin, right)) {
                Ok(text) => return Ok((text, right)),
                Err(error) => {
                    last_error = error;
                    if right == 0 {
                        break;
                    }
                    right /= 4;
                }
            }
        }
        return Err(last_error);
    };

    // AX 选区偏移与窗口文本的坐标系可能不一致（Obsidian 阅读模式），按优先级
    // 给共享锚点探测提供候选：窗口偏移 → 文档级偏移 → 0
    let anchor_candidates = |window_loc: isize| -> [isize; 3] {
        [range.location - window_loc, range.location, 0]
    };

    const WINDOW_MARGIN: isize = 1024;
    let loc = std::cmp::max(0, range.location - WINDOW_MARGIN);
    let (context, achieved_right) = fetch_context(WINDOW_MARGIN, -1)?;
    let (capture, touched_edge) = match capture_in_context(&context, word, &anchor_candidates(loc), loc, true) {
        Some(result) => result,
        None => {
            // 诊断：锚点定位失败时记录坐标系关键参数，便于分析各提供方的偏移语义
            return Err(format!(
                "no validated anchor for the selected text in the context \
                 (range [{}, {}), context {} utf16 units, word {} utf16 units, \
                 {} occurrence(s) in context, char_count {:?})",
                range.location,
                range.location + range.length,
                context.encode_utf16().count(),
                word.encode_utf16().count(),
                context.match_indices(word).count(),
                char_count
            ));
        }
    };

    // 触边重试：句子起点==窗口起点（且窗口未对齐文本开头）或终点==窗口终点，
    // 说明窗口可能截断了句子，用更大窗口重试（右缘只增不减）；仍触边则接受截断句。
    // 仅当右缘曾被收缩（提供方拒绝过更宽的窗口）且句子仍触边时才记日志——
    // 句子恰好在文档末尾结束是正常触边，不报警
    if touched_edge {
        const RETRY_WINDOW_MARGIN: isize = 8192;
        let retry_loc = std::cmp::max(0, range.location - RETRY_WINDOW_MARGIN);
        let mut final_capture = capture;
        let mut final_right = achieved_right;
        let mut final_context_len = context.encode_utf16().count();
        let mut final_touched = true;
        if let Ok((retry_context, retry_right)) = fetch_context(RETRY_WINDOW_MARGIN, achieved_right)
        {
            if let Some((retry_capture, retry_touched)) = capture_in_context(
                &retry_context,
                word,
                &anchor_candidates(retry_loc),
                retry_loc,
                true,
            ) {
                final_capture = retry_capture;
                final_right = retry_right;
                final_context_len = retry_context.encode_utf16().count();
                final_touched = retry_touched;
            }
        }
        if final_touched && final_right < WINDOW_MARGIN {
            log::warn!(
                "sentence may be truncated by the provider: right margin shrunk to {}, \
                 range [{}, {}), context {} utf16 units",
                final_right,
                range.location,
                range.location + range.length,
                final_context_len
            );
        }
        return Ok(final_capture);
    }
    return Ok(capture);
}

/// 读取焦点元素当前选区的 CFRange（AXSelectedTextRange 属性，UTF-16 码元计）。
fn get_selected_text_range_by_ax(
    focused: &core_foundation::base::CFType,
) -> Result<core_foundation::base::CFRange, String> {
    use core_foundation::base::{CFRange, CFType, CFTypeRef, TCFType};
    use core_foundation::string::CFString;

    unsafe {
        let range_attribute = CFString::from_static_string(ax_ffi::SELECTED_TEXT_RANGE_ATTRIBUTE);
        let mut range_ref: CFTypeRef = std::ptr::null();
        let err = ax_ffi::AXUIElementCopyAttributeValue(
            focused.as_CFTypeRef(),
            range_attribute.as_concrete_TypeRef(),
            &mut range_ref,
        );
        if err != 0 || range_ref.is_null() {
            return Err(format!("failed to get selected text range: AXError {err}"));
        }
        // AXUIElementCopyAttributeValue 遵循 Copy 规则（返回已 retain 的对象）。
        let range_value: CFType = TCFType::wrap_under_create_rule(range_ref);
        let mut range = CFRange { location: 0, length: 0 };
        let decoded = ax_ffi::AXValueGetValue(
            range_value.as_CFTypeRef(),
            ax_ffi::AX_VALUE_CF_RANGE_TYPE,
            &mut range as *mut CFRange as *mut std::ffi::c_void,
        );
        if decoded == 0 {
            return Err("failed to decode CFRange from AXValue".to_string());
        }
        if range.location < 0 || range.length <= 0 {
            return Err(format!(
                "invalid selected text range: location {}, length {}",
                range.location, range.length
            ));
        }
        return Ok(range);
    }
}

/// 用 AXStringForRange 参数化属性读取焦点元素在 range（UTF-16 码元计）内的文本。
fn copy_string_for_range_by_ax(
    focused: &core_foundation::base::CFType,
    range: core_foundation::base::CFRange,
) -> Result<String, String> {
    use core_foundation::base::{CFGetTypeID, CFRange, CFType, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    unsafe {
        // AXValueCreate 遵循 Create 规则（返回已 retain 的对象）。
        let range_value_ref = ax_ffi::AXValueCreate(
            ax_ffi::AX_VALUE_CF_RANGE_TYPE,
            &range as *const CFRange as *const std::ffi::c_void,
        );
        if range_value_ref.is_null() {
            return Err("failed to create AXValue for CFRange".to_string());
        }
        let range_value: CFType = TCFType::wrap_under_create_rule(range_value_ref);

        let attribute =
            CFString::from_static_string(ax_ffi::STRING_FOR_RANGE_PARAMETERIZED_ATTRIBUTE);
        let mut text_ref: CFTypeRef = std::ptr::null();
        let err = ax_ffi::AXUIElementCopyParameterizedAttributeValue(
            focused.as_CFTypeRef(),
            attribute.as_concrete_TypeRef(),
            range_value.as_CFTypeRef(),
            &mut text_ref,
        );
        if err != 0 || text_ref.is_null() {
            return Err(format!("failed to get string for range: AXError {err}"));
        }
        if CFGetTypeID(text_ref) != CFString::type_id() {
            return Err("string for range is not a string".to_string());
        }
        let text: CFString = TCFType::wrap_under_create_rule(text_ref as CFStringRef);
        return Ok(text.to_string());
    }
}

/// 模拟 Cmd+C 读取剪贴板的 AppleScript：
/// 备份剪贴板 → 临时静音系统提示音 → 模拟 Cmd+C → 轮询剪贴板变化计数 →
/// 读取剪贴板 → 恢复剪贴板与音量。
///
/// 输出协议：首字符为状态——`0` 表示剪贴板在超时内没有变化（应用没理会 Cmd+C，
/// 或没有选中内容），`1` 表示复制成功，其后紧跟所选文本。用状态码而不是空串
/// 表示“未变”，Rust 侧才能按触发场景分流（见 get_selected_text_by_applescript）。
///
/// 轮询而不是固定 delay：每 15ms 查一次 changeCount，最长约 300ms——慢应用不会
/// 因固定等待太短读到旧内容，快应用也不用白等。检测到变化后再留 30ms 收尾：
/// 有的应用先 clearContents（计数已变）、稍后才写入数据，立即读取会读到空。
///
/// 剪贴板备份/恢复是尽力而为的：剪贴板当前内容不是文本（如图片、文件）时
/// `the clipboard` 读取失败，此时跳过备份与恢复、只做复制读取——牺牲“剪贴板
/// 无痕”换取回退成功率（代价是此类场景下原剪贴板内容会被复制的文本覆盖）。
const APPLE_SCRIPT: &str = r#"
use AppleScript version "2.4"
use scripting additions
use framework "Foundation"
use framework "AppKit"

set savedAlertVolume to alert volume of (get volume settings)

-- Back up clipboard contents (best-effort; fails for non-text clipboard, e.g. images):
set clipboardSaved to false
try
    set savedClipboard to the clipboard
    set clipboardSaved to true
end try

set thePasteboard to current application's NSPasteboard's generalPasteboard()
set theCount to thePasteboard's changeCount()

tell application "System Events"
    set volume alert volume 0
end tell

-- Copy selected text to clipboard:
tell application "System Events" to keystroke "c" using {command down}

-- Poll the pasteboard change count (up to ~300ms) instead of a fixed delay:
set didChange to false
repeat 20 times
    if thePasteboard's changeCount() is not theCount then
        set didChange to true
        exit repeat
    end if
    delay 0.015
end repeat

tell application "System Events"
    set volume alert volume savedAlertVolume
end tell

if not didChange then
    return "0"
end if

-- Some apps clear the pasteboard first and write the data a moment later:
delay 0.03

set theSelectedText to the clipboard as text

if clipboardSaved then
    try
        set the clipboard to savedClipboard
    end try
end if

return "1" & theSelectedText
"#;

/// 通过 AppleScript 模拟 Cmd+C 并读取剪贴板，获取当前选中的文本。
///
/// `silent_if_unchanged`：剪贴板在模拟 Cmd+C 后未变化的语义。true（AX 能正常响应、
/// 只是没读到选区）= 什么都没选，返回空串由调用方提示“没有选区”；false（AX 查询
/// 本身失败）= 没有任何一路能证明目标应用里有选区，剪贴板不变更可能是取词目标
/// 取错了、焦点在安全输入框、或应用不理会模拟按键，返回 Err 报取词失败。
fn get_selected_text_by_applescript(silent_if_unchanged: bool) -> Result<String, String> {
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(APPLE_SCRIPT)
        .output()
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    if output.status.success() {
        let content = String::from_utf8(output.stdout)
            .map_err(|e| format!("osascript output is not valid utf-8: {e}"))?;
        return parse_applescript_output(&content, silent_if_unchanged);
    }
    let error = String::from_utf8_lossy(&output.stderr).into_owned();
    return Err(format!("osascript failed: {error}"));
}

/// 解析 APPLE_SCRIPT 的输出协议（首字符状态码 + 所选文本）。
fn parse_applescript_output(output: &str, silent_if_unchanged: bool) -> Result<String, String> {
    if let Some(text) = output.strip_prefix('1') {
        return Ok(text.trim().to_string());
    }
    if output.trim() == "0" {
        if silent_if_unchanged {
            return Ok(String::new());
        }
        return Err(
            "the clipboard did not change after the simulated Cmd+C (the target app may not \
             have been the one we queried, the focus may be in a secure input field, or the app \
             ignores the simulated keystroke)"
                .to_string(),
        );
    }
    return Err(format!("unexpected osascript output: {output:?}"));
}

#[cfg(test)]
mod tests {
    use super::{
        find_element_with_selection_by_ax, get_focused_ui_element_by_ax,
        get_selected_context_by_ax, get_selected_text_by_ax, parse_applescript_output,
        APPLE_SCRIPT,
    };

    /// AppleScript 输出协议：成功取文本、未变按触发语义分流、协议外输出报错
    #[test]
    fn applescript_output_parsing() {
        assert_eq!(parse_applescript_output("1hello world\n", true), Ok("hello world".into()));
        // 所选文本本身以 "0"/"1" 开头不应被误判（状态码只看首字符）
        assert_eq!(parse_applescript_output("10 apples\n", false), Ok("0 apples".into()));
        // 剪贴板未变：AX 正常（silent）→ 空串；AX 报错（strict）→ Err
        assert_eq!(parse_applescript_output("0\n", true), Ok(String::new()));
        assert!(parse_applescript_output("0\n", false).is_err());
        assert!(parse_applescript_output("", true).is_err());
        assert!(parse_applescript_output("garbage", true).is_err());
    }

    /// 冒烟测试：FFI 调用路径不应 panic，结果取决于辅助功能权限与当前焦点元素。
    #[test]
    fn ax_ffi_smoke_test() {
        let result = get_selected_text_by_ax();
        eprintln!("get_selected_text_by_ax() -> {result:?}");
    }

    /// 冒烟测试：取句 AX 链路不应 panic，结果取决于辅助功能权限与当前焦点元素。
    /// 只测 AX 路径：完整的 get_selected_context 会回退 AppleScript 模拟 Cmd+C 按键，
    /// 不适合在测试中运行。
    #[test]
    fn selected_context_by_ax_smoke_test() {
        let result = get_selected_context_by_ax();
        eprintln!("get_selected_context_by_ax() -> {result:?}");
    }

    /// 冒烟测试：AX 树搜索不应 panic，结果取决于辅助功能权限与当前焦点窗口。
    #[test]
    fn ax_tree_search_smoke_test() {
        if let Ok(focused) = get_focused_ui_element_by_ax() {
            let result = find_element_with_selection_by_ax(&focused);
            eprintln!(
                "find_element_with_selection_by_ax() -> {:?}",
                result.map(|found| found.map(|(_, word)| word))
            );
        }
    }

    /// 实机诊断（需环境变量 AX_TEST_PID=目标应用 pid，且其中已选中文字）：
    /// 在其焦点窗口里找 AXWebArea，跑 text marker 取句链路并打印结果。
    #[test]
    fn marker_context_by_pid_diagnostic() {
        use core_foundation::base::{CFType, TCFType};
        let Some(pid) = std::env::var("AX_TEST_PID").ok().and_then(|v| v.parse::<i32>().ok())
        else {
            return;
        };
        unsafe {
            let app: CFType =
                TCFType::wrap_under_create_rule(super::ax_ffi::AXUIElementCreateApplication(pid));
            let window =
                super::copy_element_attribute_by_ax(&app, super::ax_ffi::FOCUSED_WINDOW_ATTRIBUTE)
                    .expect("no focused window");
            fn find_role(
                e: &core_foundation::base::CFType,
                role: &str,
                depth: usize,
            ) -> Option<core_foundation::base::CFType> {
                if depth > 20 {
                    return None;
                }
                let r = super::copy_string_attribute_by_ax(e, "AXRole").unwrap_or_default();
                if r == role {
                    return Some(e.clone());
                }
                let children = super::copy_children_array_by_ax(e.as_CFTypeRef()).ok()?;
                let count = unsafe { super::ax_ffi::CFArrayGetCount(children.as_CFTypeRef()) };
                for i in 0..count {
                    let child: CFType = unsafe {
                        let raw = super::ax_ffi::CFArrayGetValueAtIndex(children.as_CFTypeRef(), i);
                        TCFType::wrap_under_get_rule(raw)
                    };
                    if let Some(found) = find_role(&child, role, depth + 1) {
                        return Some(found);
                    }
                }
                None
            }
            let web = find_role(&window, "AXWebArea", 0).expect("no AXWebArea");
            let result = super::marker_selected_context(&web);
            eprintln!("marker_selected_context(web area) -> {result:?}");
            let text = find_role(&web, "AXStaticText", 0).expect("no AXStaticText");
            let result = super::marker_selected_context(&text);
            eprintln!("marker_selected_context(static text) -> {result:?}");
        }
    }

    /// 选中文本在页面文本中的定位：忽略空白差异、记录全部出现位置、UTF-16 偏移
    #[test]
    fn selection_occurrences_in_page_text() {
        let page: Vec<u16> = "Use abundant data. Abundant? An abun dant word, abundant".encode_utf16().collect();
        // 区分大小写；空白差异不影响匹配（"abun dant" 命中 "abundant"）
        let found = super::find_selection_occurrences(&page, "abundant");
        let text = String::from_utf16_lossy(&page);
        let slices: Vec<String> = found
            .iter()
            .map(|(start, end)| String::from_utf16_lossy(&page[*start..*end]))
            .collect();
        assert_eq!(slices, vec!["abundant", "abun dant", "abundant"], "{found:?} in {text:?}");
        // 选区本身含换行（跨行选择）：与页面里的空格等价
        let page: Vec<u16> = "the quick brown fox".encode_utf16().collect();
        assert_eq!(super::find_selection_occurrences(&page, "quick\nbrown"), vec![(4, 15)]);
        // astral 字符：偏移按 UTF-16 码元
        let page: Vec<u16> = "say 😀 hi".encode_utf16().collect();
        assert_eq!(super::find_selection_occurrences(&page, "😀"), vec![(4, 6)]);
        assert!(super::find_selection_occurrences(&page, "  ").is_empty());
        assert!(super::find_selection_occurrences(&page, "zebra").is_empty());
    }

    /// 以出现位置为锚点取句：单句扩展、窗口偏移不影响结果
    #[test]
    fn pdf_occurrence_capture() {
        let page: Vec<u16> =
            "Intro line.\nThe first thing is to prepare inputs with abundant information, such as features. Next one."
                .encode_utf16()
                .collect();
        let (start, end) = super::find_selection_occurrences(&page, "abundant")[0];
        let context = super::capture_pdf_occurrence(&page, start, end).expect("should capture");
        assert_eq!(
            context.text,
            "The first thing is to prepare inputs with abundant information, such as features."
        );
        assert_eq!(context.word.as_deref(), Some("abundant"));
    }

    /// 实机诊断（需 AX_TEST_PID=目标应用 pid，其 PDF 窗口里已选中文字）：走“焦点应用窗口
    /// 搜索 → 取句”链路，打印结果与耗时。
    #[test]
    fn pdf_context_by_pid_diagnostic() {
        use core_foundation::base::{CFType, TCFType};
        let Some(pid) = std::env::var("AX_TEST_PID").ok().and_then(|v| v.parse::<i32>().ok())
        else {
            return;
        };
        let started = std::time::Instant::now();
        unsafe {
            let app: CFType =
                TCFType::wrap_under_create_rule(super::ax_ffi::AXUIElementCreateApplication(pid));
            let window =
                super::copy_element_attribute_by_ax(&app, super::ax_ffi::FOCUSED_WINDOW_ATTRIBUTE)
                    .expect("no focused window");
            let found = super::find_element_with_selection_in_window(&window);
            eprintln!("window search: {:?} in {:?}", found.as_ref().map(|(_, w)| w), started.elapsed());
            let (element, word) = found.expect("no element with a selection");
            let result = super::context_from_word_and_element(&element, &word);
            eprintln!("context_from_word_and_element -> {result:?} (total {:?})", started.elapsed());
            // 应用级焦点元素入口（Preview 的正常路径：焦点元素是页面容器，需向下找页面）
            if let Ok(focused) =
                super::copy_element_attribute_by_ax(&app, super::ax_ffi::FOCUSED_UI_ELEMENT_ATTRIBUTE)
            {
                let started = std::time::Instant::now();
                let word = super::copy_string_attribute_by_ax(&focused, super::ax_ffi::SELECTED_TEXT_ATTRIBUTE)
                    .unwrap_or_default();
                let result = super::context_from_word_and_element(&focused, &word);
                eprintln!("via focused element -> {result:?} ({:?})", started.elapsed());
            }
        }
    }

    /// 实机诊断（需 AX_TEST_PID 与 AX_TEST_WORD）：模拟“Cmd+C 只拿到词”后的 PDF 页面文本补全。
    #[test]
    fn pdf_refine_by_pid_diagnostic() {
        let (Some(pid), Ok(word)) = (
            std::env::var("AX_TEST_PID").ok().and_then(|v| v.parse::<i32>().ok()),
            std::env::var("AX_TEST_WORD"),
        ) else {
            return;
        };
        let started = std::time::Instant::now();
        let result = super::refine_with_pdf_pages(pid, &word);
        eprintln!("refine_with_pdf_pages -> {result:?} ({:?})", started.elapsed());
        eprintln!("frontmost_application_pid -> {:?}", super::frontmost_application_pid());
    }

    /// 实机诊断（需 AX_TEST_PID=目标应用 pid、AX_TEST_WORD=词，可选 AX_TEST_NTH=第几处出现）：
    /// 用逐词游走得到该词的 marker 范围（等效于用户选中它），驱动生产的取句逻辑。
    /// Chromium 不接受写入选区，故用此办法验证 Edge/Chrome 的 PDF。
    #[test]
    fn marker_range_by_word_diagnostic() {
        use core_foundation::base::{CFType, TCFType};
        let (Some(pid), Ok(word)) = (
            std::env::var("AX_TEST_PID").ok().and_then(|v| v.parse::<i32>().ok()),
            std::env::var("AX_TEST_WORD"),
        ) else {
            return;
        };
        let nth: usize = std::env::var("AX_TEST_NTH").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        let app: CFType = unsafe {
            TCFType::wrap_under_create_rule(super::ax_ffi::AXUIElementCreateApplication(pid))
        };
        let focused =
            super::copy_element_attribute_by_ax(&app, super::ax_ffi::FOCUSED_UI_ELEMENT_ATTRIBUTE)
                .expect("no focused element");
        let mut cursor = super::copy_element_attribute_by_ax(&focused, "AXStartTextMarker")
            .expect("no AXStartTextMarker");
        let mut found = 0;
        for _ in 0..30_000 {
            let Ok(next) = super::copy_parameterized_attribute_by_ax(
                &focused,
                "AXNextWordEndTextMarkerForTextMarker",
                cursor.as_CFTypeRef(),
            ) else {
                break;
            };
            if let Ok(range) = super::copy_parameterized_attribute_by_ax(
                &focused,
                "AXLeftWordTextMarkerRangeForTextMarker",
                next.as_CFTypeRef(),
            ) {
                let text = super::string_for_marker_range_by_ax(&focused, &range).unwrap_or_default();
                if text.trim() == word {
                    found += 1;
                    if found == nth {
                        let started = std::time::Instant::now();
                        let result = super::find_sentence_for_marker_range(&focused, text.trim(), &range);
                        eprintln!("RESULT ({:?}): {result:?}", started.elapsed());
                        return;
                    }
                }
            }
            cursor = next;
        }
        panic!("word {word:?} (occurrence {nth}) not found");
    }

    /// 去除正文外字符：U+0002（Chromium PDF 的行尾连字符）、软连字符；普通空白与标点保留
    #[test]
    fn strip_text_artifacts_removes_hyphenation_marks() {
        assert_eq!(
            super::strip_text_artifacts("per\u{2}centage param\u{AD}eters"),
            "percentage parameters"
        );
        assert_eq!(super::strip_text_artifacts("a\tb\nc, d-e."), "a\tb\nc, d-e.");
    }

    /// 折叠空白：换行抹成的双空格合并，首尾空白去掉
    #[test]
    fn collapse_whitespace_merges_runs() {
        assert_eq!(super::collapse_whitespace("a  b \r\n c "), "a b c");
        assert_eq!(super::collapse_whitespace(""), "");
    }

    /// 截断判定：小写开头或不以句末标点（含其后的引号括号）结尾
    #[test]
    fn truncated_sentence_detection() {
        use super::looks_truncated;
        assert!(!looks_truncated("The first thing is done."));
        assert!(!looks_truncated("He said \"stop!\""));
        assert!(!looks_truncated("这是一句话。"));
        assert!(!looks_truncated("What is this?)"));
        assert!(looks_truncated("prepare inputs with abundant information, such as ")); // 左右都被截
        assert!(looks_truncated("We take a closer look at each")); // 右侧被截
        assert!(looks_truncated("scale to improve performance in recommendation presents.")); // 左侧被截
        assert!(looks_truncated("Introduction")); // 标题：无句末标点
    }

    /// 抹换行后，视觉行硬换行不再把跨行整句切碎；偏移（UTF-16）与长度保持不变
    #[test]
    fn flattened_pdf_lines_form_one_sentence() {
        use super::{capture_in_context, flatten_line_breaks, SentenceCapture};
        let raw = "3.2 Input Layer. The first thing to build a model is to\r\nprepare inputs with abundant information, such as User features\r\ninclude user ID and other user information. Next one.";
        let flat = flatten_line_breaks(raw);
        assert_eq!(flat.encode_utf16().count(), raw.encode_utf16().count());
        let offset = raw.find("abundant").map(|byte| raw[..byte].encode_utf16().count()).unwrap();
        // 不抹换行：句子被换行切碎，只剩 "prepare inputs ... features"
        let (broken, _) = capture_in_context(raw, "abundant", &[offset as isize], 1, false).unwrap();
        match broken {
            SentenceCapture::Expanded(s) => assert!(!s.contains("The first thing"), "{s:?}"),
            _ => panic!("expected Expanded"),
        }
        let (capture, _) = capture_in_context(&flat, "abundant", &[offset as isize], 1, false).unwrap();
        match capture {
            SentenceCapture::Expanded(s) => assert_eq!(
                s,
                "The first thing to build a model is to  prepare inputs with abundant information, such as User features  include user ID and other user information."
            ),
            _ => panic!("expected Expanded"),
        }
    }

    /// APPLE_SCRIPT 应能被 osacompile 编译（仅语法检查，不执行脚本）。
    #[test]
    fn apple_script_syntax_check() {
        let target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&target_dir).expect("failed to create target dir");
        let output_path = target_dir.join("apple-script-syntax-check.scpt");
        let output = std::process::Command::new("osacompile")
            .arg("-e")
            .arg(APPLE_SCRIPT)
            .arg("-o")
            .arg(&output_path)
            .output()
            .expect("failed to run osacompile");
        let _ = std::fs::remove_file(&output_path);
        assert!(
            output.status.success(),
            "osacompile failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
