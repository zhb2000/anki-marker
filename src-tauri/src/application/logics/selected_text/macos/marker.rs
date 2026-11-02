//! text marker 提供方（AXTextMarker，如 Safari 网页、Chromium 系）的取词与取句链路。

use super::super::capture::{
    block_looks_cut, can_join_lines, capture_in_segments, SentenceCapture,
};
use super::super::SelectedContext;
use super::ax::{copy_element_attribute_by_ax, copy_parameterized_attribute_by_ax};
use super::ax_ffi;
use super::text::{looks_truncated, strip_text_artifacts, strip_text_artifacts_with_offset};

/// 行级提供方的行导航扩展半径（行数）：覆盖绝大多数跨行句，同时限制病态文档的读取量。
/// 真正的块边界由拼行规则决定，这里只是上界。
const LINE_EXPANSION_LINES: usize = 40;

/// 校验对象确实是 AXTextMarkerRange。AXTextMarkerRangeCopyStart/EndMarker 在本进程内
/// 解析对象内部结构（不是跨进程调用），而属性值的类型由目标应用决定，非 WebKit/Chromium
/// 的应用可能返回别的 CF 类型；不校验就解包是未定义行为，最坏让本应用崩溃。
fn ensure_text_marker_range(range: &core_foundation::base::CFType) -> Result<(), String> {
    use core_foundation::base::{CFGetTypeID, TCFType};

    let is_marker_range =
        unsafe { CFGetTypeID(range.as_CFTypeRef()) == ax_ffi::AXTextMarkerRangeGetTypeID() };
    return if is_marker_range {
        Ok(())
    } else {
        Err("the value is not an AXTextMarkerRange".to_string())
    };
}

/// 取 marker 范围的起点/终点 marker（Copy 规则包装，调用方负责释放）。
fn marker_of_range(
    range: &core_foundation::base::CFType,
    end: bool,
) -> Result<core_foundation::base::CFType, String> {
    use core_foundation::base::TCFType;

    ensure_text_marker_range(range)?;
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
}

/// 段落之后是否接着同一句话：段落没有结束句子，且下一个行块以续接形态开头。
///
/// WebKit 会把行内 MathML 公式、图片等内嵌对象切成独立的块，段落文本到公式处就
/// 断了——此时“段尾”不是句子边界，必须继续拼行才能拿回完整句子。标题与表格
/// 单元格的下一块以大写字母开头，这里返回 false，保持“段落即块”的行为。
fn continues_after_paragraph(
    element: &core_foundation::base::CFType,
    paragraph: &core_foundation::base::CFType,
) -> bool {
    use core_foundation::array::CFArray;
    use core_foundation::base::TCFType;

    let Ok(paragraph_text) = string_for_marker_range_by_ax(element, paragraph) else {
        return false;
    };
    // 段落被切断的两种形态：
    // 1. 停在内嵌对象前的空白上（"…hidden states "）；
    // 2. 停在引用标注之类的文本片段边界上（"…long short-term memory ["，PDF 里
    //    "[13]" 往往自成一段）。段落不会以开括号/连字符结尾，出现即说明被截断。
    // 标题、表格单元格两种形态都不满足，因此不会被误判
    if !block_looks_cut(&paragraph_text) {
        return false;
    }
    let Ok(paragraph_end) = marker_of_range(paragraph, true) else {
        return false;
    };
    let Ok(next_end) = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::NEXT_LINE_END_MARKER_PARAMETERIZED_ATTRIBUTE,
        paragraph_end.as_CFTypeRef(),
    ) else {
        return false;
    };
    let markers = CFArray::from_CFTypes(&[paragraph_end.clone(), next_end]);
    let Ok(range) = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::RANGE_FOR_UNORDERED_MARKERS_PARAMETERIZED_ATTRIBUTE,
        markers.as_CFTypeRef(),
    ) else {
        return false;
    };
    let Ok(next_text) = string_for_marker_range_by_ax(element, &range) else {
        return false;
    };
    return can_join_lines(&paragraph_text, &next_text);
}

/// 读取元素的选区 marker 范围（AXSelectedTextMarkerRange），并校验其类型。
fn copy_selected_marker_range_by_ax(
    element: &core_foundation::base::CFType,
) -> Result<core_foundation::base::CFType, String> {
    let range =
        copy_element_attribute_by_ax(element, ax_ffi::SELECTED_TEXT_MARKER_RANGE_ATTRIBUTE)?;
    ensure_text_marker_range(&range)?;
    return Ok(range);
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
/// 经 AXStringForTextMarkerRange 读文本）。元素没有该属性、或读取文本失败返回 None，
/// 只有确实读到空串才返回 Some("")——读取失败不能当成“没有选区”，否则调用方会跳过
/// 树搜索与后台重试，并把随后的取词失败误报成“没有选区”。
pub(super) fn marker_selected_text(element: &core_foundation::base::CFType) -> Option<String> {
    let range = copy_selected_marker_range_by_ax(element).ok()?;
    return match string_for_marker_range_by_ax(element, &range) {
        Ok(text) => Some(text),
        Err(error) => {
            log::info!("failed to read the text of the selected marker range: {error}");
            None
        }
    };
}

/// text marker 提供方的选词取句：选区 marker 范围 → 词；所在段落的 marker 范围 → 上下文；
/// “段首到选区起点”的前缀范围文本长度 → 选区在段内的 UTF-16 偏移 → 共享 capture
/// 校验锚点并切句（与 Windows UIA 的前缀法同构）。
///
/// 返回 None = 不是此类提供方（调用方走其他链路）；Some(空文本) = 是但没有选区；
/// 取句失败降级为仅录词原文。不用提供方自带的句子范围
/// （AXSentenceTextMarkerRangeForTextMarker），以保持各应用的分句规则一致。
pub(super) fn marker_selected_context(
    element: &core_foundation::base::CFType,
) -> Option<SelectedContext> {
    let word = marker_selected_text(element)?;
    if word.trim().is_empty() {
        return Some(SelectedContext {
            text: word,
            word: None,
            word_offset: None,
        });
    }
    log::info!("found the selection via the text marker API");
    return Some(match find_sentence_for_marker_selection(element, &word) {
        Ok((
            SentenceCapture::Expanded {
                sentence,
                word_offset,
            },
            capture_word,
        )) => {
            let (text, word_offset) = strip_text_artifacts_with_offset(&sentence, word_offset);
            SelectedContext {
                text,
                word: Some(strip_text_artifacts(capture_word.trim())),
                word_offset,
            }
        }
        Ok((SentenceCapture::SelectionAsIs(text), _)) => SelectedContext {
            text: strip_text_artifacts(&text),
            word: None,
            word_offset: None,
        },
        Err(error) => {
            log::warn!("failed to extract sentence for the selected word: {error}");
            SelectedContext {
                text: strip_text_artifacts(&word),
                word: None,
                word_offset: None,
            }
        }
    });
}

/// text marker 提供方的取句：先用选区起点所在“段落”作上下文；段落过短导致句子被截断、
/// 或段落装不下整个选区（Chromium 的 PDF 查看器里“段落”只是一个文本片段）时，
/// 改用行导航向前后扩展上下文重试。返回 {取句结果, 应上报的词}——词在抹换行的
/// 重试路径下与句子保持同一文本形态。
fn find_sentence_for_marker_selection(
    element: &core_foundation::base::CFType,
    word: &str,
) -> Result<(SentenceCapture, String), String> {
    let selection = copy_selected_marker_range_by_ax(element)?;
    return find_sentence_for_marker_range(element, word, &selection);
}

/// find_sentence_for_marker_selection 的取句部分：选区 marker 范围已知。
///
/// 取句单元是“块”，不是“窗口”：
/// 1. 选区起点所在段落。段落不含换行时就是完整块（网页内容的标题、表格单元格、
///    列表项都不带句末标点，但它们是完整的块），直接取句，不做任何扩展；
/// 2. 段落含换行时是行级提供方（Chromium 的 PDF 查看器：段落 = 整段，但文本按
///    视觉行给出，行间是换行）。UAX #29 会在换行处强制分句，直接取句只能得到一行，
///    所以先按“块内拼行、块间不拼”的规则重建块（capture::join_block_around_anchor）
///    再取句——这样跨行整句能取回，而相邻的标题/图注不会被并进来；
/// 3. 段落本身取不到块（选区跨段、或以文本片段为段落的提供方）时，才退到行导航
///    扩展：向外扩 LINE_EXPANSION_LINES 行，同样按拼行规则重建块。
fn find_sentence_for_marker_range(
    element: &core_foundation::base::CFType,
    word: &str,
    selection: &core_foundation::base::CFType,
) -> Result<(SentenceCapture, String), String> {
    use core_foundation::base::{CFType, TCFType};

    let selection_start = marker_of_range(selection, false)?;
    let selection_end = marker_of_range(selection, true)?;

    // 第一轮：选区起点所在段落
    let paragraph = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::PARAGRAPH_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
        selection_start.as_CFTypeRef(),
    )?;
    let paragraph_start = marker_of_range(&paragraph, false)?;
    let paragraph_capture = capture_in_paragraph(
        element,
        word,
        &paragraph,
        &paragraph_start,
        &selection_start,
    );

    // 段落被内嵌对象（MathML 公式、图片等）截断时，段落边界不等于句子边界：
    // 若下一块是续接形态（小写/数字/标点/公式字形），交给行扩展把句子接回来。
    // 标题、表格单元格的下一块以大写开头，不会被误判成截断
    let block_is_trusted = match &paragraph_capture {
        Ok((capture, _)) => {
            let truncated = matches!(
                capture,
                SentenceCapture::Expanded { sentence, .. } if looks_truncated(sentence)
            );
            !truncated || !continues_after_paragraph(element, &paragraph)
        }
        Err(_) => false,
    };
    if block_is_trusted {
        return paragraph_capture;
    }

    // 第二轮：段落取不到块时，退到行导航扩展
    let line_start_of = |marker: &CFType, end: bool| -> Result<CFType, String> {
        let line = copy_parameterized_attribute_by_ax(
            element,
            ax_ffi::LINE_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
            marker.as_CFTypeRef(),
        )?;
        return marker_of_range(&line, end);
    };
    let (anchor_left, anchor_right) = match (
        line_start_of(&selection_start, false),
        line_start_of(&selection_end, true),
    ) {
        (Ok(left), Ok(right)) => (left, right),
        (Err(error), _) | (_, Err(error)) => {
            return Err(format!("{error}; line navigation unavailable"));
        }
    };

    return match expand_block_around_lines(
        element,
        word,
        &anchor_left,
        &anchor_right,
        &selection_start,
    ) {
        Ok(result) => Ok(result),
        Err(error) => Err(format!(
            "{}; line context: {error}",
            paragraph_capture.err().unwrap_or_default()
        )),
    };
}

/// 逐行收集选区所在的块（用于读区间不插分隔符的提供方，如 Chromium 系）。
///
/// 从选区所在行向两侧逐行步进：每读一行就与相邻行做一次拼行判定（复用
/// capture::can_join_lines），满足才继续外扩。返回块文本（行间以换行连接）与
/// 选区起点在块内的 UTF-16 偏移，交给 join_block_around_anchor 统一拼接与去连字符。
fn collect_block_by_lines(
    element: &core_foundation::base::CFType,
    anchor_left: &core_foundation::base::CFType,
    anchor_right: &core_foundation::base::CFType,
    selection_start: &core_foundation::base::CFType,
) -> Result<(Vec<String>, usize, usize), String> {
    // 选区所在的连续文本（手动跨行选择时可能包含多行，但 Chromium 不插换行）
    let anchor_text = string_for_marker_range_by_ax(
        element,
        &marker_range_between(element, anchor_left, anchor_right)?,
    )?;
    let intra = prefix_length_utf16(element, anchor_left, selection_start)?;

    // 向前：逐行取上一行，直到拼行规则判定为块边界
    let mut before: Vec<String> = Vec::new();
    let mut cursor = anchor_left.clone();
    let mut head = anchor_text.clone();
    for _ in 0..LINE_EXPANSION_LINES {
        let Some(previous) = previous_line_start(element, &cursor) else {
            break;
        };
        let Some(text) = line_text(element, &previous, &cursor) else {
            break;
        };
        if !can_join_lines(&text, &head) {
            break;
        }
        head = text.clone();
        before.push(text);
        cursor = previous;
    }
    before.reverse();

    // 向后：逐行取下一行
    let mut after: Vec<String> = Vec::new();
    let mut cursor = anchor_right.clone();
    let mut tail = anchor_text.clone();
    for _ in 0..LINE_EXPANSION_LINES {
        let Some(next) = next_line_end(element, &cursor) else {
            break;
        };
        let Some(text) = line_text(element, &cursor, &next) else {
            break;
        };
        if !can_join_lines(&tail, &text) {
            break;
        }
        tail = text.clone();
        after.push(text);
        cursor = next;
    }

    let anchor_index = before.len();
    before.push(anchor_text);
    before.extend(after);
    return Ok((before, anchor_index, intra));
}

/// 两个 marker 之间的范围对象。
fn marker_range_between(
    element: &core_foundation::base::CFType,
    from: &core_foundation::base::CFType,
    to: &core_foundation::base::CFType,
) -> Result<core_foundation::base::CFType, String> {
    use core_foundation::array::CFArray;
    use core_foundation::base::TCFType;

    let markers = CFArray::from_CFTypes(&[from.clone(), to.clone()]);
    return copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::RANGE_FOR_UNORDERED_MARKERS_PARAMETERIZED_ATTRIBUTE,
        markers.as_CFTypeRef(),
    );
}

/// 两个 marker 之间的文本；读不到返回 None（调用方按走查结束处理）。
fn line_text(
    element: &core_foundation::base::CFType,
    from: &core_foundation::base::CFType,
    to: &core_foundation::base::CFType,
) -> Option<String> {
    let range = marker_range_between(element, from, to).ok()?;
    return string_for_marker_range_by_ax(element, &range).ok();
}

/// 从给定 marker 起上一行的行首 marker。
fn previous_line_start(
    element: &core_foundation::base::CFType,
    marker: &core_foundation::base::CFType,
) -> Option<core_foundation::base::CFType> {
    use core_foundation::base::TCFType;

    let next = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::PREVIOUS_LINE_START_MARKER_PARAMETERIZED_ATTRIBUTE,
        marker.as_CFTypeRef(),
    )
    .ok()?;
    return (next.as_CFTypeRef() != marker.as_CFTypeRef()).then_some(next);
}

/// 从给定 marker 起下一行的行尾 marker。
fn next_line_end(
    element: &core_foundation::base::CFType,
    marker: &core_foundation::base::CFType,
) -> Option<core_foundation::base::CFType> {
    use core_foundation::base::TCFType;

    let next = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::NEXT_LINE_END_MARKER_PARAMETERIZED_ATTRIBUTE,
        marker.as_CFTypeRef(),
    )
    .ok()?;
    return (next.as_CFTypeRef() != marker.as_CFTypeRef()).then_some(next);
}

/// 在段落范围内取句：段落作为“一个片段”交给共享层，含换行时由拼行规则合并。
fn capture_in_paragraph(
    element: &core_foundation::base::CFType,
    word: &str,
    paragraph: &core_foundation::base::CFType,
    paragraph_start: &core_foundation::base::CFType,
    selection_start: &core_foundation::base::CFType,
) -> Result<(SentenceCapture, String), String> {
    /// 上下文的 UTF-16 码元上限：防御病态页面（无块级分隔的巨型文本节点），
    /// 超限降级为仅录词，不为切一句话读取整页文本
    const MAX_CONTEXT_UTF16_UNITS: usize = 50_000;

    let context = string_for_marker_range_by_ax(element, paragraph)?;
    let context_units = context.encode_utf16().count();
    if context_units > MAX_CONTEXT_UTF16_UNITS {
        return Err(format!(
            "the paragraph is too long ({context_units} utf16 units), skipping sentence extraction"
        ));
    }
    let sel_start_utf16 = prefix_length_utf16(element, paragraph_start, selection_start)?;
    return capture_in_segments(&[context], 0, sel_start_utf16, word);
}

/// 以选区所在行为锚逐行收集块，再交给共享层取句。
fn expand_block_around_lines(
    element: &core_foundation::base::CFType,
    word: &str,
    anchor_left: &core_foundation::base::CFType,
    anchor_right: &core_foundation::base::CFType,
    selection_start: &core_foundation::base::CFType,
) -> Result<(SentenceCapture, String), String> {
    // 逐行收集，而不是读一个大范围：Chromium 系读区间时不插任何分隔符（实测跨 14 行
    // 的 1245 个字符里 0 个换行），WebKit 的行结构也只在逐行读取时才最可靠。
    let (segments, anchor_index, intra) =
        collect_block_by_lines(element, anchor_left, anchor_right, selection_start)?;
    return capture_in_segments(&segments, anchor_index, intra, word);
}

fn prefix_length_utf16(
    element: &core_foundation::base::CFType,
    from: &core_foundation::base::CFType,
    to: &core_foundation::base::CFType,
) -> Result<usize, String> {
    use core_foundation::array::CFArray;
    use core_foundation::base::TCFType;

    let markers = CFArray::from_CFTypes(&[from.clone(), to.clone()]);
    let prefix = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::RANGE_FOR_UNORDERED_MARKERS_PARAMETERIZED_ATTRIBUTE,
        markers.as_CFTypeRef(),
    )?;
    return Ok(string_for_marker_range_by_ax(element, &prefix)?
        .encode_utf16()
        .count());
}

/// 范围链路切出的句子被截断时，用 marker 链路重试：元素没有 marker 范围、或重试仍没有
/// 得到一个完整的句子，都返回 None（沿用范围链路的结果）。
pub(super) fn refine_truncated_with_markers(
    element: &core_foundation::base::CFType,
    word: &str,
) -> Option<SelectedContext> {
    copy_selected_marker_range_by_ax(element).ok()?;
    return match find_sentence_for_marker_selection(element, word) {
        Ok((
            SentenceCapture::Expanded {
                sentence,
                word_offset,
            },
            capture_word,
        )) if !looks_truncated(&sentence) => {
            log::info!(
                "the range chain's sentence looked truncated, refined via the text marker API"
            );
            let (text, word_offset) = strip_text_artifacts_with_offset(&sentence, word_offset);
            Some(SelectedContext {
                text,
                word: Some(strip_text_artifacts(capture_word.trim())),
                word_offset,
            })
        }
        _ => None,
    };
}

#[cfg(test)]
mod tests {
    use super::super::ax::{copy_children_array_by_ax, copy_string_attribute_by_ax};

    /// 实机诊断（需环境变量 AX_TEST_PID=目标应用 pid，且其中已选中文字）：
    /// 在其焦点窗口里找 AXWebArea，跑 text marker 取句链路并打印结果。
    #[test]
    fn marker_context_by_pid_diagnostic() {
        use core_foundation::base::{CFType, TCFType};
        let Some(pid) = std::env::var("AX_TEST_PID")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
        else {
            return;
        };
        unsafe {
            let app: CFType =
                TCFType::wrap_under_create_rule(super::ax_ffi::AXUIElementCreateApplication(pid));
            // 取窗口：优先焦点窗口；后台启动的浏览器可能不报焦点窗口，退到第一个窗口
            let window =
                super::copy_element_attribute_by_ax(&app, super::ax_ffi::FOCUSED_WINDOW_ATTRIBUTE)
                    .or_else(|_| {
                        let windows = super::copy_element_attribute_by_ax(&app, "AXWindows")?;
                        let count = super::ax_ffi::CFArrayGetCount(windows.as_CFTypeRef());
                        if count <= 0 {
                            return Err("no window".to_string());
                        }
                        let raw = super::ax_ffi::CFArrayGetValueAtIndex(windows.as_CFTypeRef(), 0);
                        return Ok(TCFType::wrap_under_get_rule(raw));
                    })
                    .expect("no window");
            fn find_role(
                e: &core_foundation::base::CFType,
                role: &str,
                depth: usize,
            ) -> Option<core_foundation::base::CFType> {
                if depth > 20 {
                    return None;
                }
                let r = copy_string_attribute_by_ax(e, "AXRole").unwrap_or_default();
                if r == role {
                    return Some(e.clone());
                }
                let children = copy_children_array_by_ax(e.as_CFTypeRef()).ok()?;
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
            // PDF 查看器的选区挂在插件元素（AXGroup）上，不在 web area：也跑一遍
            if let Ok(focused) = super::copy_element_attribute_by_ax(
                &app,
                super::ax_ffi::FOCUSED_UI_ELEMENT_ATTRIBUTE,
            ) {
                let result = super::marker_selected_context(&focused);
                eprintln!("marker_selected_context(focused) -> {result:?}");
            }
            let web = find_role(&window, "AXWebArea", 0).expect("no AXWebArea");
            let result = super::marker_selected_context(&web);
            eprintln!("marker_selected_context(web area) -> {result:?}");
            let text = find_role(&web, "AXStaticText", 0).expect("no AXStaticText");
            let result = super::marker_selected_context(&text);
            eprintln!("marker_selected_context(static text) -> {result:?}");
        }
    }

    /// 实机诊断（需 AX_TEST_PID=目标应用 pid、AX_TEST_WORD=词，可选 AX_TEST_NTH=第几处出现）：
    /// 用逐词游走得到该词的 marker 范围（等效于用户选中它），驱动生产的取句逻辑。
    /// Chromium 不接受写入选区，故用此办法验证 Edge/Chrome 的 PDF。
    #[test]
    fn marker_range_by_word_diagnostic() {
        use core_foundation::base::{CFType, TCFType};
        let (Some(pid), Ok(word)) = (
            std::env::var("AX_TEST_PID")
                .ok()
                .and_then(|v| v.parse::<i32>().ok()),
            std::env::var("AX_TEST_WORD"),
        ) else {
            return;
        };
        let nth: usize = std::env::var("AX_TEST_NTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
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
                let text =
                    super::string_for_marker_range_by_ax(&focused, &range).unwrap_or_default();
                if text.trim() == word {
                    found += 1;
                    if found == nth {
                        let started = std::time::Instant::now();
                        let result =
                            super::find_sentence_for_marker_range(&focused, text.trim(), &range);
                        eprintln!("RESULT ({:?}): {result:?}", started.elapsed());
                        return;
                    }
                }
            }
            cursor = next;
        }
        panic!("word {word:?} (occurrence {nth}) not found");
    }

    /// marker 范围解包前的类型校验：真正的 AXTextMarkerRange 通过，其他 CF 类型被拒
    #[test]
    fn text_marker_range_type_check() {
        use core_foundation::base::{CFType, CFTypeRef, TCFType};
        use core_foundation::boolean::CFBoolean;
        use core_foundation::string::CFString;

        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXTextMarkerCreate(
                allocator: CFTypeRef,
                bytes: *const u8,
                length: isize,
            ) -> CFTypeRef;
            fn AXTextMarkerRangeCreate(
                allocator: CFTypeRef,
                start: CFTypeRef,
                end: CFTypeRef,
            ) -> CFTypeRef;
        }

        let bytes = [1u8, 2, 3, 4];
        let range: CFType = unsafe {
            let marker: CFType = TCFType::wrap_under_create_rule(AXTextMarkerCreate(
                std::ptr::null(),
                bytes.as_ptr(),
                bytes.len() as isize,
            ));
            TCFType::wrap_under_create_rule(AXTextMarkerRangeCreate(
                std::ptr::null(),
                marker.as_CFTypeRef(),
                marker.as_CFTypeRef(),
            ))
        };
        assert!(super::ensure_text_marker_range(&range).is_ok());
        assert!(super::ensure_text_marker_range(&CFString::new("x").as_CFType()).is_err());
        assert!(super::ensure_text_marker_range(&CFBoolean::true_value().as_CFType()).is_err());
    }
}
