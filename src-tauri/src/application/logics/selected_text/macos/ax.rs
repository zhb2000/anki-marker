//! AX 辅助功能 API 的基础封装：系统级/应用元素的创建与消息超时、焦点元素与焦点窗口、
//! 各类属性读取，以及在焦点窗口的无障碍树中搜索带选区的文本元素。

use super::ax_ffi;

/// AX 跨进程调用的超时（秒）。系统默认约 6 秒且作用于每一次调用：目标应用卡死时，主路径
/// 要依次等焦点查询、PDF 精修的窗口查询，用户十几秒后才有反馈。超时后走既有的降级链路
///（回退模拟复制、后台重试），一次慢调用不会丢结果。
const AX_MESSAGING_TIMEOUT_SECONDS: f32 = 1.0;

/// 创建系统级元素（Create 规则，随 Drop 释放），并设置本进程全局的 AX 消息超时——
/// 对系统级元素设置才是全局的，取词链路之后拿到的子元素都受它约束。
fn create_system_wide_element() -> core_foundation::base::CFType {
    use core_foundation::base::{CFType, TCFType};

    unsafe {
        let system_wide: CFType =
            TCFType::wrap_under_create_rule(ax_ffi::AXUIElementCreateSystemWide());
        ax_ffi::AXUIElementSetMessagingTimeout(
            system_wide.as_CFTypeRef(),
            AX_MESSAGING_TIMEOUT_SECONDS,
        );
        return system_wide;
    }
}

/// 创建目标应用的元素（Create 规则，随 Drop 释放）并设置 AX 消息超时：系统级元素的全局
/// 设置生效前（如焦点查询之前）也不会被卡死的应用拖住。
pub(super) fn create_application_element(pid: i32) -> Option<core_foundation::base::CFType> {
    use core_foundation::base::{CFType, TCFType};

    unsafe {
        let app_ref = ax_ffi::AXUIElementCreateApplication(pid);
        if app_ref.is_null() {
            return None;
        }
        let app: CFType = TCFType::wrap_under_create_rule(app_ref);
        ax_ffi::AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), AX_MESSAGING_TIMEOUT_SECONDS);
        return Some(app);
    }
}

/// 获取系统当前焦点 UI 元素。
///
/// 注意：未授予辅助功能权限时此处会失败，由调用方回退或提示。
pub(super) fn get_focused_ui_element_by_ax() -> Result<core_foundation::base::CFType, String> {
    use core_foundation::base::{CFTypeRef, TCFType};
    use core_foundation::string::CFString;

    unsafe {
        let system_wide = create_system_wide_element();

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
pub(super) fn copy_string_attribute_by_ax(
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
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::string::CFString;

    unsafe {
        let attribute_ref = CFString::from_static_string(attribute);
        let mut value_ref: CFTypeRef = std::ptr::null();
        let err = ax_ffi::AXUIElementCopyAttributeValue(
            element,
            attribute_ref.as_concrete_TypeRef(),
            &mut value_ref,
        );
        if err != 0 || value_ref.is_null() {
            return Err(format!(
                "failed to copy attribute \"{attribute}\": AXError {err}"
            ));
        }
        // 先接管所有权（Copy 规则）再判类型：类型不符提前返回时对象也会随 Drop 释放
        let value: CFType = TCFType::wrap_under_create_rule(value_ref);
        let Some(value) = value.downcast_into::<CFString>() else {
            return Err(format!(
                "value of attribute \"{attribute}\" is not a string"
            ));
        };
        return Ok(value.to_string());
    }
}

/// 读取辅助功能元素的引用型属性（如 AXWindow、AXFocusedWindow），返回未解析的 CFType。
pub(super) fn copy_element_attribute_by_ax(
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
            return Err(format!(
                "failed to copy attribute \"{attribute}\": AXError {err}"
            ));
        }
        // AXUIElementCopyAttributeValue 遵循 Copy 规则（返回已 retain 的对象）。
        return Ok(TCFType::wrap_under_create_rule(value_ref));
    }
}

/// 读取辅助功能元素的子元素数组（AXChildren，Create 规则返回，随 Drop 释放）。
pub(super) fn copy_children_array_by_ax(
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
        // 先接管所有权（Copy 规则）再判类型：类型不符提前返回时对象也会随 Drop 释放
        let value: CFType = TCFType::wrap_under_create_rule(value_ref);
        if CFGetTypeID(value.as_CFTypeRef()) != CFArray::<CFType>::type_id() {
            return Err(format!(
                "value of attribute \"{}\" is not an array",
                ax_ffi::CHILDREN_ATTRIBUTE
            ));
        }
        return Ok(value);
    }
}

/// 读取辅助功能元素的整型属性值（如 AXNumberOfCharacters）；
/// AX 报错、空指针、值非数字或数字超出 i64 时返回 Err。
pub(super) fn copy_i64_attribute_by_ax(
    element: &core_foundation::base::CFType,
    attribute: &'static str,
) -> Result<i64, String> {
    use core_foundation::base::{CFType, CFTypeRef, TCFType};
    use core_foundation::number::CFNumber;
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
            return Err(format!(
                "failed to copy attribute \"{attribute}\": AXError {err}"
            ));
        }
        // 先接管所有权（Copy 规则）再判类型：类型不符提前返回时对象也会随 Drop 释放
        let value: CFType = TCFType::wrap_under_create_rule(value_ref);
        let Some(value) = value.downcast_into::<CFNumber>() else {
            return Err(format!(
                "value of attribute \"{attribute}\" is not a number"
            ));
        };
        return value
            .to_i64()
            .ok_or_else(|| format!("value of attribute \"{attribute}\" does not fit in i64"));
    }
}

/// 参数化属性读取（Copy 规则，返回值随 Drop 释放）；AX 报错或空指针返回 Err。
pub(super) fn copy_parameterized_attribute_by_ax(
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
            return Err(format!(
                "failed to copy parameterized attribute \"{attribute}\": AXError {err}"
            ));
        }
        return Ok(TCFType::wrap_under_create_rule(value_ref));
    }
}

/// 读取元素所属应用的进程 id（AXUIElementGetPid），失败返回 None。
pub(super) fn get_element_pid(element: &core_foundation::base::CFType) -> Option<i32> {
    use core_foundation::base::TCFType;

    let mut pid: i32 = 0;
    let err = unsafe { ax_ffi::AXUIElementGetPid(element.as_CFTypeRef(), &mut pid) };
    return if err == 0 && pid > 0 { Some(pid) } else { None };
}

/// 读取当前焦点应用的焦点窗口（AX 树搜索的备选根）。
fn get_focused_window_by_ax() -> Result<core_foundation::base::CFType, String> {
    let system_wide = create_system_wide_element();
    let app = copy_element_attribute_by_ax(&system_wide, ax_ffi::FOCUSED_APPLICATION_ATTRIBUTE)?;
    return copy_element_attribute_by_ax(&app, ax_ffi::FOCUSED_WINDOW_ATTRIBUTE);
}

/// 在焦点元素所在窗口的 AX 子树中搜索持有非空选区的元素（入口：推导搜索根）。
pub(super) fn find_element_with_selection_by_ax(
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
pub(super) fn find_element_with_selection_in_window(
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
        if let Ok(text) = copy_string_attribute_by_ax_ref(element, ax_ffi::SELECTED_TEXT_ATTRIBUTE)
        {
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
pub(super) fn frontmost_application_pid() -> Option<i32> {
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

#[cfg(test)]
mod tests {
    use super::{find_element_with_selection_by_ax, get_focused_ui_element_by_ax};

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
}
