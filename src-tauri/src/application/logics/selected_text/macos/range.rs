//! 标准 AX 文本元素的取句链路：经 AXSelectedTextRange（CFRange）与 AXStringForRange
//! 读取选区周边的上下文窗口并切句。

use super::super::capture::{capture_in_context, SentenceCapture};
use super::ax::copy_i64_attribute_by_ax;
use super::ax_ffi;

/// 取句主链路：读选区 CFRange → 窗口化读上下文 → 跨句判断 → 分句 → 触边重试 → 校验。
/// 任一环节失败返回 Err，由调用方降级为“仅录入词原文”。
pub(super) fn find_sentence_for_word_by_ax(
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
        CFRange {
            location: loc,
            length,
        }
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
    let anchor_candidates =
        |window_loc: isize| -> [isize; 3] { [range.location - window_loc, range.location, 0] };

    const WINDOW_MARGIN: isize = 1024;
    let loc = std::cmp::max(0, range.location - WINDOW_MARGIN);
    let (context, achieved_right) = fetch_context(WINDOW_MARGIN, -1)?;
    let (capture, touched_edge) =
        match capture_in_context(&context, word, &anchor_candidates(loc), loc, true) {
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
        let mut range = CFRange {
            location: 0,
            length: 0,
        };
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
    use core_foundation::base::{CFRange, CFType, CFTypeRef, TCFType};
    use core_foundation::string::CFString;

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
        // 先接管所有权（Copy 规则）再判类型：类型不符提前返回时对象也会随 Drop 释放
        let value: CFType = TCFType::wrap_under_create_rule(text_ref);
        let Some(text) = value.downcast_into::<CFString>() else {
            return Err("string for range is not a string".to_string());
        };
        return Ok(text.to_string());
    }
}
