//! 获取系统当前选中的文本（目前仅 macOS）。
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
//! 选词取句（word-to-sentence）开启时入口为 get_selected_context：
//! AX 先读选中的词，再经 AXSelectedTextRange / AXStringForRange 读取词所在的
//! 上下文窗口并切出整个句子；选区横跨多个句子时（用户手动跨句/跨行选择）
//! 原样录入所选文本——只扩展、不丢弃；取句链路任一环节失败时降级为仅录入
//! 词原文，与既有的 AX 成功路径语义一致。

#[cfg(target_os = "macos")]
pub fn get_selected_text() -> Result<String, String> {
    if let Ok(text) = get_selected_text_by_ax() {
        if !text.trim().is_empty() {
            return Ok(text);
        }
        // AX “成功”但为空：可能是真的没选中文本，也可能是目标应用不通过 AX
        // 暴露选区（如 VSCode），无法区分，统一回退到模拟 Cmd+C 再判一次
    }
    return get_selected_text_by_applescript();
}

#[cfg(not(target_os = "macos"))]
pub fn get_selected_text() -> Result<String, String> {
    return Err("获取选中文本暂不支持此平台".to_string());
}

/// 划词捕获结果：text 为录入文本（取句成功时是整句），word 为取句模式命中的单词。
#[derive(Debug)]
pub struct SelectedContext {
    pub text: String,
    pub word: Option<String>,
}

/// word_to_sentence 为 true 时尝试“选词取句”：AX 全链路成功返回 {句子, 词}；
/// AX 拿到词但取句失败返回 {词原文, None}；AX 报错或词为空时回退 AppleScript（同现有逻辑）。
/// word_to_sentence 为 false 时完全等同现有 get_selected_text 的行为。
///
/// `on_retry_captured`：AX 失败回退后，若目标应用的无障碍树在后台异步物化
/// （如 Word），后台重试取到完整句子时经此回调补发结果（见 spawn_selection_retry）。
#[cfg(target_os = "macos")]
pub fn get_selected_context(
    word_to_sentence: bool,
    on_retry_captured: impl FnOnce(SelectedContext) + Send + 'static,
) -> Result<SelectedContext, String> {
    if !word_to_sentence {
        return get_selected_text().map(|text| SelectedContext { text, word: None });
    }
    match get_selected_context_by_ax() {
        Ok(context) if !context.text.trim().is_empty() => return Ok(context),
        Ok(_) => {
            // AX “成功”但为空：无法区分“真没选”与“应用不暴露选区”（如 VSCode 的
            // Monaco 编辑器），回退模拟 Cmd+C 再判一次；记日志便于诊断各应用的 AX 暴露情况
            log::warn!(
                "AX path returned empty selected text (no selection or the app does not \
                 expose its selection via AX), falling back to AppleScript"
            );
        }
        Err(ax_error) => {
            // AX 失败（含树搜索无果）：立即回退 AppleScript 保证录入不阻塞；
            // 若拿到了目标应用 pid，同时启动后台重试——无障碍树异步物化后补发句子
            if let Some(pid) = ax_error.retry_pid {
                log::info!("spawning background AX selection retry for pid {pid}");
                spawn_selection_retry(pid, on_retry_captured);
            }
            log::warn!("AX path failed, falling back to AppleScript: {}", ax_error.message);
        }
    }
    return get_selected_text_by_applescript().map(|text| SelectedContext { text, word: None });
}

#[cfg(not(target_os = "macos"))]
pub fn get_selected_context(
    _word_to_sentence: bool,
    _on_retry_captured: impl FnOnce(SelectedContext) + Send + 'static,
) -> Result<SelectedContext, String> {
    return Err("获取选中文本暂不支持此平台".to_string());
}

/// macOS 辅助功能 C API（ApplicationServices 框架）的最小 FFI 绑定。
///
/// 仅绑定读取选中文本与选词取句所需的符号，避免引入 `accessibility-ng`/`cocoa`
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
}

/// 获取系统当前焦点 UI 元素。
///
/// 注意：未授予辅助功能权限时此处会失败，由调用方回退或提示。
#[cfg(target_os = "macos")]
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
        return Ok(TCFType::wrap_under_create_rule(focused_ref));
    }
}

/// 读取辅助功能元素的字符串属性值；AX 报错、空指针或值非字符串时返回 Err。
#[cfg(target_os = "macos")]
fn copy_string_attribute_by_ax(
    element: &core_foundation::base::CFType,
    attribute: &'static str,
) -> Result<String, String> {
    use core_foundation::base::TCFType;

    return copy_string_attribute_by_ax_ref(element.as_CFTypeRef(), attribute);
}

/// copy_string_attribute_by_ax 的 CFTypeRef 版本（供树搜索直接操作借用的元素引用）。
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
fn get_selected_text_by_ax() -> Result<String, String> {
    let focused = get_focused_ui_element_by_ax()?;
    return copy_string_attribute_by_ax(&focused, ax_ffi::SELECTED_TEXT_ATTRIBUTE);
}

/// 取句结果：单句时为用户所选词扩展出的整句；用户手动跨句选择时为其所选原文。
#[cfg(target_os = "macos")]
enum SentenceCapture {
    /// 选区落在单个句子内，已扩展为整句（前端可预选命中的单词）
    Expanded(String),
    /// 选区横跨多个句子：只扩展、不丢弃——原样录入用户所选文本（不做单词预选）
    SelectionAsIs(String),
}

/// AX 取句链路的失败：携带诊断信息与（可能存在的）后台重试句柄（目标应用 pid）。
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
fn get_selected_context_by_ax() -> Result<SelectedContext, AxAttemptError> {
    let focused = get_focused_ui_element_by_ax().map_err(|error| AxAttemptError {
        message: error,
        retry_pid: None,
    })?;
    let word = match copy_string_attribute_by_ax(&focused, ax_ffi::SELECTED_TEXT_ATTRIBUTE) {
        Ok(word) => word,
        Err(focused_error) => {
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

/// 读取元素所属应用的进程 id（AXUIElementGetPid），失败返回 None。
#[cfg(target_os = "macos")]
fn get_element_pid(element: &core_foundation::base::CFType) -> Option<i32> {
    use core_foundation::base::TCFType;

    let mut pid: i32 = 0;
    let err = unsafe { ax_ffi::AXUIElementGetPid(element.as_CFTypeRef(), &mut pid) };
    return if err == 0 && pid > 0 { Some(pid) } else { None };
}

/// 拿到所选文本后的收尾：对持有选区的元素跑取句链路，失败降级为仅录词原文。
#[cfg(target_os = "macos")]
fn context_from_word_and_element(
    element: &core_foundation::base::CFType,
    word: &str,
) -> SelectedContext {
    return match find_sentence_for_word_by_ax(element, word) {
        Ok(SentenceCapture::Expanded(sentence)) => SelectedContext {
            text: sentence,
            word: Some(word.trim().to_string()),
        },
        Ok(SentenceCapture::SelectionAsIs(text)) => SelectedContext { text, word: None },
        Err(error) => {
            // 已拿到词：取句失败不致命，降级为现有 AX 成功路径的语义（录入词原文）
            log::warn!("failed to extract sentence for the selected word: {error}");
            SelectedContext { text: word.to_string(), word: None }
        }
    };
}

/// 读取当前焦点应用的焦点窗口（AX 树搜索的备选根）。
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
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

/// 后台重试的互斥标记：0 表示无重试在途，否则为正在重试的应用 pid。
#[cfg(target_os = "macos")]
static RETRYING_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// 后台重试：部分应用（实测 Word）的无障碍树在首次被查询后才异步物化——
/// 首次划词时是只有百余节点的桩树，反复查询几秒后树才建成（所以第二次划词
/// 才成功）。此函数在后台按固定间隔轮询目标应用（pid 不随前台切换失效）：
/// 唤醒属性 → 树搜索 → 取句链路，树就绪后取到句子并通过 `on_captured` 补发。
/// 总轮询窗口约 RETRY_ATTEMPTS * RETRY_INTERVAL_MS，超时放弃。
#[cfg(target_os = "macos")]
pub fn spawn_selection_retry<F>(pid: i32, on_captured: F)
where
    F: FnOnce(SelectedContext) + Send + 'static,
{
    use core_foundation::base::{CFType, TCFType};
    use std::sync::atomic::Ordering;

    /// 轮询次数与间隔（总计约 3 秒，覆盖实测的树物化耗时）
    const RETRY_ATTEMPTS: usize = 5;
    const RETRY_INTERVAL_MS: u64 = 600;

    // 同一时间只允许一个重试在途，避免连按快捷键叠加多个轮询线程
    if RETRYING_PID.swap(pid, Ordering::SeqCst) != 0 {
        return;
    }
    std::thread::spawn(move || {
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
                    log::info!("AX selection retry succeeded on attempt {attempt}");
                    let context = context_from_word_and_element(&element, &word);
                    RETRYING_PID.store(0, Ordering::SeqCst);
                    on_captured(context);
                    return;
                }
            }
        }
        RETRYING_PID.store(0, Ordering::SeqCst);
    });
}

/// 取句主链路：读选区 CFRange → 窗口化读上下文 → 跨句判断 → 分句 → 触边重试 → 校验。
/// 任一环节失败返回 Err，由调用方降级为“仅录入词原文”。
#[cfg(target_os = "macos")]
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

    const WINDOW_MARGIN: isize = 1024;
    let loc = std::cmp::max(0, range.location - WINDOW_MARGIN);
    let (context, achieved_right) = fetch_context(WINDOW_MARGIN, -1)?;
    let (capture, touched_edge) = match capture_in_context(&context, word, range, loc) {
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
            if let Some((retry_capture, retry_touched)) =
                capture_in_context(&retry_context, word, range, retry_loc)
            {
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

/// 在上下文中定位选区并切句：锚点探测（校验门控）→ 跨句保留 → 单句扩展。
///
/// 返回切句结果，以及句子是否触及上下文边缘（触边意味着窗口可能截断了句子，
/// 调用方值得扩大窗口重试；跨句保留路径无此概念，恒为 false）。
#[cfg(target_os = "macos")]
fn capture_in_context(
    context: &str,
    word: &str,
    range: core_foundation::base::CFRange,
    window_loc: isize,
) -> Option<(SentenceCapture, bool)> {
    let sel_start_utf16 = find_anchor(context, word, range, window_loc)?;
    let sel_end_utf16 = sel_start_utf16 + word.encode_utf16().count();

    // 只扩展、不丢弃：选区横跨多个句子时原样录入用户所选文本，不做分句截断。
    // 典型场景：Chrome PDF 中手动选中跨行的完整句子——PDF 视觉行间的 \n 使
    // UAX #29（SB4：CR/LF 后强制分句）把整句切为多行碎片，若仍取"与选区起点
    // 相交的句子"就只会录入第一行，丢弃了用户明确选择的内容
    if super::sentence::count_intersecting_sentences(context, sel_start_utf16, sel_end_utf16) > 1 {
        return Some((SentenceCapture::SelectionAsIs(word.trim().to_string()), false));
    }
    let (sentence, sent_start, sent_end) =
        super::sentence::find_sentence_with_range(context, sel_start_utf16, sel_end_utf16)?;
    let touched_edge = (sent_start == 0 && window_loc > 0) || sent_end == context.len();
    return Some((SentenceCapture::Expanded(sentence), touched_edge));
}

/// 在上下文中定位所选文本的 UTF-16 起点（锚点探测，校验门控）。
///
/// 浏览器 AXWebArea 等提供方的偏移坐标系存在已知怪癖：AXSelectedTextRange 给出的
/// 偏移与 AXStringForRange 返回文本的坐标系可能不一致（例如偏移是文档级的、返回
/// 文本却是块级的）。因此不盲信偏移，按优先级尝试候选锚点：窗口偏移（提供方忠实
/// 响应请求窗口）→ 文档级偏移（提供方忽略窗口起点、从文档开头返回文本）→ 0
/// （提供方从选区开始返回）→ 所选文本的唯一出现位置（零次出现无法定位、多次出现
/// 有歧义，均不采用）。前三种以“该位置的 UTF-16 子串等于所选文本”校验后才采用。
/// 全部失败返回 None——宁可降级为仅录词，也不切出错误的句子。
#[cfg(target_os = "macos")]
fn find_anchor(
    context: &str,
    word: &str,
    range: core_foundation::base::CFRange,
    window_loc: isize,
) -> Option<usize> {
    let context_utf16: Vec<u16> = context.encode_utf16().collect();
    let word_utf16: Vec<u16> = word.encode_utf16().collect();
    if word_utf16.is_empty() {
        return None;
    }
    // 校验：candidate 起点的 UTF-16 子串与所选文本逐码元相等
    let matches_at = |candidate: isize| -> bool {
        if candidate < 0 {
            return false;
        }
        let start = candidate as usize;
        return start + word_utf16.len() <= context_utf16.len()
            && context_utf16[start..start + word_utf16.len()] == word_utf16[..];
    };
    for candidate in [range.location - window_loc, range.location, 0] {
        if matches_at(candidate) {
            return Some(candidate as usize);
        }
    }
    let mut occurrences = context.match_indices(word);
    return match (occurrences.next(), occurrences.next()) {
        (Some((byte_index, _)), None) => Some(context[..byte_index].encode_utf16().count()),
        _ => None,
    };
}

/// 读取焦点元素当前选区的 CFRange（AXSelectedTextRange 属性，UTF-16 码元计）。
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{
        capture_in_context, find_anchor, find_element_with_selection_by_ax,
        get_focused_ui_element_by_ax, get_selected_context_by_ax, get_selected_text_by_ax,
        SentenceCapture, APPLE_SCRIPT,
    };
    use core_foundation::base::CFRange;

    fn range_at(location: isize, length: isize) -> CFRange {
        CFRange { location, length }
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

    /// 锚点候选 1：提供方忠实响应请求窗口（窗口偏移有效）
    #[test]
    fn anchor_window_offset() {
        // 文档 "Hello world. This is a test."，窗口 loc=9，word "This" 在文档偏移 13
        let context = "ld. This is a te";
        // 候选 1：13 - 9 = 4，context[4..8] == "This"
        assert_eq!(find_anchor(context, "This", range_at(13, 4), 9), Some(4));
    }

    /// 锚点候选 2：提供方忽略窗口起点、从文档开头返回文本（文档级偏移）
    #[test]
    fn anchor_document_offset() {
        let context = "Hello world. This is";
        // 候选 1（13-9=4）命中 "o wo" 不匹配；候选 2（13）命中 "This"
        assert_eq!(find_anchor(context, "This", range_at(13, 4), 9), Some(13));
    }

    /// 锚点候选 3：提供方从选区开始返回文本
    #[test]
    fn anchor_selection_start() {
        let context = "This is a test.";
        // 候选 1（4）与候选 2（13，越界）均失败；候选 3（0）命中
        assert_eq!(find_anchor(context, "This", range_at(13, 4), 9), Some(0));
    }

    /// 锚点候选 4：偏移完全失真时靠唯一出现位置定位；多处出现有歧义返回 None
    #[test]
    fn anchor_unique_occurrence() {
        let context = "jumped over. The fox runs. End.";
        assert_eq!(
            find_anchor(context, "fox", range_at(9999, 3), 9000),
            Some("jumped over. The ".encode_utf16().count())
        );
        // 两次出现 → 歧义，不采用（上下文开头放非匹配内容，避免候选 3 先命中）
        assert_eq!(find_anchor("a fox and fox", "fox", range_at(9999, 3), 9000), None);
    }

    /// 所选文本不在上下文中：返回 None（降级为仅录词，不切错句）
    #[test]
    fn anchor_not_found() {
        assert_eq!(find_anchor("Hello world.", "zebra", range_at(0, 5), 0), None);
    }

    /// 含 emoji（UTF-16 代理对）的所选文本：按 UTF-16 码元校验与定位
    #[test]
    fn anchor_with_emoji() {
        // "say " 共 4 个 UTF-16 码元，😀 占 2 个
        assert_eq!(find_anchor("say 😀 hi.", "😀", range_at(4, 2), 0), Some(4));
    }

    /// 块级上下文取句（Obsidian 阅读模式场景）：range 是文档级偏移、
    /// AXStringForRange 只返回段落块，靠锚点探测取到块内完整句子
    #[test]
    fn capture_in_block_context() {
        let context = "First sentence here. The target word is fox.";
        let (capture, touched_edge) = capture_in_context(context, "fox", range_at(5000, 3), 4000)
            .expect("should capture");
        match capture {
            SentenceCapture::Expanded(sentence) => {
                assert_eq!(sentence, "The target word is fox.");
            }
            _ => panic!("expected Expanded"),
        }
        // 句子终点==上下文终点 → 触边（调用方会扩大窗口重试一次）
        assert!(touched_edge);
    }

    /// 跨句选择保留原文（只扩展、不丢弃），且不触发触边重试
    #[test]
    fn capture_selection_as_is() {
        let context = "One. Two three. Four.";
        let word = "Two three. Four"; // 用户手动跨句选择
        let (capture, touched_edge) = capture_in_context(context, word, range_at(5, 15), 0)
            .expect("should capture");
        match capture {
            SentenceCapture::SelectionAsIs(text) => assert_eq!(text, word),
            _ => panic!("expected SelectionAsIs"),
        }
        assert!(!touched_edge);
    }
}

/// 模拟 Cmd+C 读取剪贴板的 AppleScript：
/// 备份剪贴板 → 临时静音系统提示音 → 模拟 Cmd+C → 读取剪贴板 → 恢复剪贴板与音量。
///
/// 剪贴板备份/恢复是尽力而为的：剪贴板当前内容不是文本（如图片、文件）时
/// `the clipboard` 读取失败，此时跳过备份与恢复、只做复制读取——牺牲“剪贴板
/// 无痕”换取回退成功率（代价是此类场景下原剪贴板内容会被复制的文本覆盖）。
#[cfg(target_os = "macos")]
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
delay 0.1 -- Without this, the clipboard may have stale data.

tell application "System Events"
    set volume alert volume savedAlertVolume
end tell

if thePasteboard's changeCount() is theCount then
    return ""
end if

set theSelectedText to the clipboard

if clipboardSaved then
    try
        set the clipboard to savedClipboard
    end try
end if

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
