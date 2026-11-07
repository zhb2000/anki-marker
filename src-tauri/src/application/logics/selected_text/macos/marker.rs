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

/// 短于此长度的“段落”只可能是行内节点（链接、强调片段），不是真正的块：
/// 这类片段值得做一次树上溯，且容器文本与它相等时可以穿过行内节点继续上溯
const MAX_INLINE_FRAGMENT_CHARS: usize = 200;

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
    // 段落后面没有同一行的内容：段落边界就是真边界（标题、表格单元格、段落末尾）
    if next_text.trim().is_empty() {
        return false;
    }
    // 段落被切断的形态：
    // 1. 停在空白或开括号上（行内公式、引用链接："…hidden states "、"…memory ["）；
    // 2. 停在单词中间（折行把词切开："…em‐bedding" + "s (Mikolov…"），此时段尾是
    //    普通字母，后面的续接以小写字母/数字开头
    if block_looks_cut(&paragraph_text) {
        return true;
    }
    let last_is_word = paragraph_text
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric);
    let next_starts_mid_word = next_text
        .chars()
        .find(|c| !c.is_whitespace())
        .is_some_and(|c| c.is_lowercase() || c.is_numeric());
    return last_is_word && next_starts_mid_word;
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
            context: Default::default(),
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
                context,
            },
            capture_word,
        )) => {
            let (text, word_offset) = strip_text_artifacts_with_offset(&sentence, word_offset);
            SelectedContext {
                text,
                word: Some(strip_text_artifacts(capture_word.trim())),
                word_offset,
                context,
            }
        }
        Ok((SentenceCapture::SelectionAsIs(text), _)) => SelectedContext {
            context: Default::default(),
            text: strip_text_artifacts(&text),
            word: None,
            word_offset: None,
        },
        Err(error) => {
            log::warn!("failed to extract sentence for the selected word: {error}");
            SelectedContext {
                context: Default::default(),
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
    use core_foundation::base::TCFType;

    let selection_start = marker_of_range(selection, false)?;
    let selection_end = marker_of_range(selection, true)?;

    // 第一轮：选区起点所在段落
    let paragraph = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::PARAGRAPH_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
        selection_start.as_CFTypeRef(),
    )?;
    let paragraph_start = marker_of_range(&paragraph, false)?;

    // 段落被行内链接截断（"…word embeddings ("）：链接文字不在 marker 文本流里，
    // 段落和行范围都在链接处停下。这时改从 AX 树拼出完整段落再取句
    if let Ok(paragraph_text) = string_for_marker_range_by_ax(element, &paragraph) {
        // 段落被切断（行内链接、公式、或折行把单词切开）：只要还有内容可补，
        // 就尝试从 AX 树拼完整段落——行文本在链接处同样截断，且带折行连字符
        // 片段短于行内节点上限时，无论首字符是什么都做一次树上溯：大写专名（链接、
        // 斜体）与正常块开头的形态完全一样，靠字符判不出来
        let cont = paragraph_text.trim().chars().count() < MAX_INLINE_FRAGMENT_CHARS
            || starts_mid_block(&paragraph_text)
            || continues_after_paragraph(element, &paragraph);
        if cont {
            if let Ok(provider_start) =
                prefix_length_utf16(element, &paragraph_start, &selection_start)
            {
                // 同名文字可能在树里出现多次（信息框、目录、参考文献）：逐个候选尝试，
                // 首选能切出完整句子的那个
                let mut complete = Vec::new();
                let mut fallback = None;
                for (assembled, fragment_offset) in
                    block_candidates_from_tree(element, &paragraph_text, selection)
                {
                    // 提供方的偏移是“被截断的段落文本”坐标系里的；树拼文本把链接文字
                    // 补了回来，选区落在链接之后时两者不再对齐，需要重新定位
                    let sel_start = offset_in_assembled_text(
                        &assembled,
                        &paragraph_text,
                        provider_start,
                        fragment_offset,
                        word,
                    );
                    match capture_in_segments(&[assembled], 0, sel_start, word) {
                        Ok((capture, capture_word)) => {
                            let truncated = matches!(
                                &capture,
                                SentenceCapture::Expanded { sentence, .. } if looks_truncated(sentence)
                            );
                            if truncated {
                                fallback.get_or_insert((capture, capture_word));
                            } else {
                                complete.push((capture, capture_word, fragment_offset, sel_start));
                            }
                        }
                        Err(error) => {
                            log::info!("rebuilding the paragraph from the AX tree failed: {error}")
                        }
                    }
                }
                // 同名文字出现在多处时可能有好几个“成句”的候选：分不清就退回词本身，
                // 不拿一句位置不对的句子冒充
                if complete.len() == 1 {
                    let (capture, capture_word, fragment_offset, sel_start) =
                        complete.pop().expect("checked length");
                    log::info!(
                        "rebuilt the block from the AX tree (fragment at {fragment_offset}, \
                         selection at {sel_start})"
                    );
                    return Ok((capture, capture_word));
                }
                if complete.len() > 1 {
                    log::info!(
                        "several blocks contain the selected text ({} candidates); keeping the word only",
                        complete.len()
                    );
                } else if let Some(found) = fallback {
                    log::info!("rebuilt the block from the AX tree (the sentence looks truncated)");
                    // 树拼块也只到链接处（PDF 把同一视觉行按行内链接切成多个组）：改用
                    // 逐行收集再取一次——行拼接规则能跨过这些结构切口，而树拼块本身
                    // 已经到顶。行块的句子更长就采用它
                    if let Ok((wider, wider_word)) =
                        line_block(element, word, &selection_start, &selection_end)
                    {
                        if sentence_len(&wider) > sentence_len(&found.0) {
                            log::info!("line navigation recovered a longer sentence than the AX tree block");
                            return Ok((wider, wider_word));
                        }
                    }
                    return Ok(found);
                }
            }
        }
    }

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
    return match line_block(element, word, &selection_start, &selection_end) {
        Ok(result) => Ok(result),
        Err(error) => Err(format!(
            "{}; line context: {error}",
            paragraph_capture.err().unwrap_or_default()
        )),
    };
}

/// 用选区所在行向外逐行收集块并取句（行级提供方的兜底路径）。
fn line_block(
    element: &core_foundation::base::CFType,
    word: &str,
    selection_start: &core_foundation::base::CFType,
    selection_end: &core_foundation::base::CFType,
) -> Result<(SentenceCapture, String), String> {
    use core_foundation::base::{CFType, TCFType};

    let line_start_of = |marker: &CFType, end: bool| -> Result<CFType, String> {
        let line = copy_parameterized_attribute_by_ax(
            element,
            ax_ffi::LINE_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
            marker.as_CFTypeRef(),
        )?;
        return marker_of_range(&line, end);
    };
    let (anchor_left, anchor_right) = match (
        line_start_of(selection_start, false),
        line_start_of(selection_end, true),
    ) {
        (Ok(left), Ok(right)) => (left, right),
        (Err(error), _) | (_, Err(error)) => {
            return Err(format!("{error}; line navigation unavailable"));
        }
    };
    return expand_block_around_lines(element, word, &anchor_left, &anchor_right, selection_start);
}

/// 结果里句子的长度（非整句结果记 0）：用来在多个候选之间挑更完整的那个
fn sentence_len(capture: &SentenceCapture) -> usize {
    return match capture {
        SentenceCapture::Expanded { sentence, .. } => sentence.chars().count(),
        SentenceCapture::SelectionAsIs(_) => 0,
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
    // 用提供方自己的“行范围”取文本：行内链接、公式在 AX 里是独立节点，我们自建的
    // marker 区间（AXTextMarkerRangeForUnorderedTextMarkers）读到链接处就截断，
    // 而提供方的行范围文本是完整的（实测 "…as visible in Fig. 2." 含链接文字）。
    let anchor_line = line_range_of(element, selection_start)
        .ok_or("the provider has no line range for the selection")?;
    let anchor_text = string_for_marker_range_by_ax(element, &anchor_line)?;
    let anchor_start = marker_of_range(&anchor_line, false).unwrap_or_else(|_| anchor_left.clone());
    let anchor_line_end =
        marker_of_range(&anchor_line, true).unwrap_or_else(|_| anchor_right.clone());
    let intra = prefix_length_utf16(element, &anchor_start, selection_start)?;

    // 向前：逐行取上一行，直到拼行规则判定为块边界
    let mut before: Vec<String> = Vec::new();
    let mut cursor = anchor_start;
    let mut head = anchor_text.clone();
    for _ in 0..LINE_EXPANSION_LINES {
        let Some(previous_start) = previous_line_start(element, &cursor) else {
            break;
        };
        let Some(previous_line) = line_range_of(element, &previous_start) else {
            break;
        };
        let Ok(text) = string_for_marker_range_by_ax(element, &previous_line) else {
            break;
        };
        if !can_join_lines(&text, &head) {
            break;
        }
        head = text.clone();
        before.push(text);
        cursor = previous_start;
    }
    before.reverse();

    // 向后：逐行取下一行
    let mut after: Vec<String> = Vec::new();
    let mut cursor = anchor_line_end;
    let mut tail = anchor_text.clone();
    for _ in 0..LINE_EXPANSION_LINES {
        let Some(next_end) = next_line_end(element, &cursor) else {
            break;
        };
        let Some(next_line) = line_range_of(element, &next_end) else {
            break;
        };
        let Ok(text) = string_for_marker_range_by_ax(element, &next_line) else {
            break;
        };
        if !can_join_lines(&tail, &text) {
            break;
        }
        tail = text.clone();
        after.push(text);
        if let Ok(line_end) = marker_of_range(&next_line, true) {
            cursor = line_end;
        } else {
            break;
        }
    }

    let anchor_index = before.len();
    before.push(anchor_text);
    before.extend(after);
    return Ok((before, anchor_index, intra));
}

/// 行内引用链接的文字不在 marker 文本流里（段落文本、行范围文本都停在 "(" 处），
/// 但 AX 树里链接节点挂着文字（AXLink 的 AXTitle，或其子 AXStaticText）。以段落片段
/// 开头为锚在树里定位那个段落组，再按子节点顺序拼出完整文本。
///
/// 只用于“段落确实被行内链接截断”的情形：正常段落不需要走树，开销也更大。
fn block_candidates_from_tree(
    element: &core_foundation::base::CFType,
    paragraph_text: &str,
    selection: &core_foundation::base::CFType,
) -> Vec<(String, usize)> {
    use super::ax::{copy_children_array_by_ax, copy_string_attribute_by_ax};
    use core_foundation::base::{CFType, TCFType};

    /// 用于在树里认领段落的开头长度：太短容易撞车，太长容易受空白差异影响
    const PREFIX_CHARS: usize = 24;
    /// 遍历预算：防御超长文档（只在段落被截断时才走这条路）
    const NODE_BUDGET: usize = 20_000;

    let fragment = paragraph_text.trim_end();
    // 片段短于认领长度时（引用串之间常常只剩 “, astronomy ” 这样的碎片）改用整段
    // 精确匹配：短前缀会撞车，但整段文本本身是唯一的
    let exact = fragment.trim().chars().count() < PREFIX_CHARS;
    if fragment.trim().is_empty() {
        return Vec::new();
    }
    let prefix: String = fragment.trim_start().chars().take(PREFIX_CHARS).collect();

    fn children(node: &CFType) -> Vec<CFType> {
        let Ok(array) = copy_children_array_by_ax(node.as_CFTypeRef()) else {
            return Vec::new();
        };
        let count = unsafe { ax_ffi::CFArrayGetCount(array.as_CFTypeRef()) };
        let mut result = Vec::new();
        for index in 0..count {
            let raw = unsafe { ax_ffi::CFArrayGetValueAtIndex(array.as_CFTypeRef(), index) };
            if !raw.is_null() {
                result.push(unsafe { TCFType::wrap_under_get_rule(raw) });
            }
        }
        return result;
    }

    fn value(node: &CFType, attribute: &'static str) -> String {
        return copy_string_attribute_by_ax(node, attribute).unwrap_or_default();
    }

    /// 深度优先收集可能是片段开头的静态文本：
    /// - 片段够长时按前缀匹配；
    /// - 片段短时按整段精确匹配（短前缀会撞车）；
    /// - 片段开头横跨多个静态文本时（`<i>Felis catus</i>` + `), also called…`），
    ///   退一步收“整段文本是片段开头一部分”的节点。
    ///
    /// 同一段文字可能出现在多处（正文与信息框都写着 “Felis”），这里全部收下，
    /// 由调用方用选区所在行验收。
    fn find_candidates(
        node: &CFType,
        prefix: &str,
        fragment: &str,
        exact: bool,
        depth: usize,
        budget: &mut usize,
        out: &mut Vec<CFType>,
    ) {
        if depth > 40 || *budget == 0 || out.len() >= MAX_CANDIDATES {
            return;
        }
        *budget -= 1;
        if value(node, "AXRole") == "AXStaticText" {
            let text = value(node, "AXValue");
            let trimmed = text.trim();
            let matched = if exact {
                trimmed == fragment.trim()
            } else {
                trimmed.starts_with(prefix)
                    || (trimmed.chars().count() >= 6 && fragment.starts_with(trimmed))
            };
            if matched
                && !out
                    .iter()
                    .any(|seen| seen.as_CFTypeRef() == node.as_CFTypeRef())
            {
                out.push(node.clone());
            }
        }
        for child in children(node) {
            find_candidates(&child, prefix, fragment, exact, depth + 1, budget, out);
        }
    }

    /// 拼一个节点的文本：静态文本取 AXValue，链接取子静态文本或 AXTitle，
    /// 其他容器递归；行内节点之间没有分隔符
    fn text_of(node: &CFType, depth: usize) -> String {
        if depth > 6 {
            return String::new();
        }
        let role = value(node, "AXRole");
        if role == "AXStaticText" {
            return value(node, "AXValue");
        }
        if role == "AXLink" {
            let inner = children(node)
                .iter()
                .map(|child| text_of(child, depth + 1))
                .collect::<String>();
            return if inner.trim().is_empty() {
                value(node, "AXTitle")
            } else {
                inner
            };
        }
        let mut inner = String::new();
        for child in children(node) {
            // 标题是块级边界：拼接时在它两侧断行，否则标题会和相邻正文连成一句
            // （文档站的短段落尤其常见）。行内节点（链接、强调）不加分隔符，
            // 它们本来就是同一句话的一部分
            if value(&child, "AXRole") == "AXHeading" {
                if !inner.is_empty() && !inner.ends_with('\n') {
                    inner.push('\n');
                }
                inner.push_str(&text_of(&child, depth + 1));
                inner.push('\n');
                continue;
            }
            inner.push_str(&text_of(&child, depth + 1));
        }
        if !inner.is_empty() {
            return inner;
        }
        return value(node, "AXValue");
    }

    /// 同一段文字可能在树里出现多次（正文与信息框都写着 “Felis”）：逐个候选尝试，
    /// 用选区所在行的文本验收，避免把别处的同名文字当成选区所在块
    const MAX_CANDIDATES: usize = 8;

    // 首选：按选区的屏幕坐标命中元素，从它上溯。同名文字在别处出现也不会拿错
    let mut blocks: Vec<(String, usize)> = Vec::new();
    let mut budget = NODE_BUDGET;
    let mut candidates: Vec<CFType> = Vec::new();
    find_candidates(
        element,
        &prefix,
        fragment,
        exact,
        0,
        &mut budget,
        &mut candidates,
    );

    let line = selection_line_text(element, selection);
    for candidate in candidates.iter().take(MAX_CANDIDATES) {
        let Some((text, offset)) = climb_to_block(candidate, fragment, &text_of, &value) else {
            continue;
        };
        // 能读到选区所在行时，块必须包含它——同名文字拼出来的块对不上
        if block_contains_line(&text, line.as_deref())
            && !blocks.iter().any(|(seen, _)| seen == &text)
        {
            blocks.push((text, offset));
        }
    }
    return blocks;
}

/// 从命中的静态文本逐层上溯，取第一个“包含段落片段、且比片段更长”的容器。
///
/// 用“包含”而不是“以片段开头”：提供方把行内链接之间的文本 run 当作段落时，
/// 片段从块的中间开始（左边是前一个链接），容器的起点不等于片段的起点。
/// 容器文本与片段相等时：行内节点（链接）可以穿过，块级角色（标题、单元格）
/// 到此为止——再往上就是小节/正文容器，扩过去只会把相邻段落拼进来。
fn climb_to_block(
    node: &core_foundation::base::CFType,
    fragment: &str,
    text_of: &dyn Fn(&core_foundation::base::CFType, usize) -> String,
    value: &dyn Fn(&core_foundation::base::CFType, &'static str) -> String,
) -> Option<(String, usize)> {
    let mut current = node.clone();
    // 只看祖先：起始节点本身就是片段（文本 run / 链接里的静态文本），不构成块
    for _ in 0..6 {
        let Ok(parent) = copy_element_attribute_by_ax(&current, "AXParent") else {
            break;
        };
        let assembled = text_of(&parent, 0);
        if assembled != fragment {
            if let Some(offset) = utf16_index_of(&assembled, fragment, 0) {
                return Some((assembled, offset));
            }
        } else if is_block_role(&value(&parent, "AXRole"))
            || fragment.chars().count() >= MAX_INLINE_FRAGMENT_CHARS
        {
            return None;
        }
        current = parent;
    }
    return None;
}

/// 块级角色：文本与片段相等即说明片段本身就是一个完整的块，不再上溯。
fn is_block_role(role: &str) -> bool {
    return matches!(
        role,
        "AXHeading" | "AXCell" | "AXRow" | "AXList" | "AXListItem" | "AXParagraph" | "AXTable"
    );
}

/// 选区所在行的文本（抹平空白与折行连字符）：用来给树拼结果做验收。
/// 行文本过短（多半也在链接处被截断）时不作为依据。
fn selection_line_text(
    element: &core_foundation::base::CFType,
    selection_start: &core_foundation::base::CFType,
) -> Option<String> {
    let line = line_range_of(element, selection_start)?;
    let text = string_for_marker_range_by_ax(element, &line).ok()?;
    let normalized = normalize_for_compare(&text);
    return (normalized.chars().count() >= 15).then_some(normalized);
}

/// 树拼结果必须包含选区所在行——同名文字在别处出现时（信息框、目录、参考文献）
/// 拼出来的块与选区所在行对不上，据此拒掉。
fn block_contains_line(block: &str, line: Option<&str>) -> bool {
    let Some(line) = line else {
        return true;
    };
    return normalize_for_compare(block).contains(line);
}

/// 比较用的归一化：折叠空白，去掉浏览器排版插入的折行连字符。
fn normalize_for_compare(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, '\u{2010}' | '\u{2011}' | '\u{00AD}'))
        .collect();
    return cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
}

/// 提供方给出的、包含该 marker 的行范围对象。
fn line_range_of(
    element: &core_foundation::base::CFType,
    marker: &core_foundation::base::CFType,
) -> Option<core_foundation::base::CFType> {
    use core_foundation::base::TCFType;

    return copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::LINE_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
        marker.as_CFTypeRef(),
    )
    .ok();
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

/// 选区起点在树拼文本里的 UTF-16 偏移。
///
/// 段落片段原样保留在树拼文本里、起点在 `fragment_offset`：选区落在片段内时，
/// 提供方报告的偏移加上这个起点即可。选区落在片段之后（被行内链接隔开）时，链接
/// 文字是树里补进来的、段落文本里没有，提供方的偏移停在切点，改从切点在树拼文本里
/// 重新定位所选文本。
///
/// 定位失败时返回推算值，交给 capture_in_segments 的逐码元校验拒掉，不在这里猜。
fn offset_in_assembled_text(
    assembled: &str,
    paragraph_text: &str,
    sel_start: usize,
    fragment_offset: usize,
    word: &str,
) -> usize {
    let fragment_units = paragraph_text.trim_end().encode_utf16().count();
    if sel_start < fragment_units {
        return fragment_offset + sel_start;
    }
    return utf16_index_of(assembled, word.trim(), fragment_offset + fragment_units)
        .unwrap_or(fragment_offset + sel_start);
}

/// 段落片段是否从块的中间开始：提供方把行内链接之间的文本 run 当作“段落”时
/// （Chromium 系网页），片段会以空白、小写字母、闭合标点开头，或者出现没有
/// 对应开括号的闭括号（引用串 “Tolan et al., 2024), astronomy (”）——块在这
/// 之前还有内容。标题、表格单元格、正常段落都不满足。
fn starts_mid_block(text: &str) -> bool {
    let Some(first) = text.chars().next() else {
        return false;
    };
    if first.is_whitespace()
        || first.is_lowercase()
        || matches!(first, ')' | ']' | '}' | ',' | ';' | ':' | '.')
    {
        return true;
    }
    let mut depth = 0i32;
    for c in text.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => {}
        }
    }
    return false;
}

/// 从给定的 UTF-16 偏移起找第一个匹配，返回匹配处的 UTF-16 偏移。
fn utf16_index_of(haystack: &str, needle: &str, from_utf16: usize) -> Option<usize> {
    let units: Vec<u16> = haystack.encode_utf16().collect();
    let needle: Vec<u16> = needle.encode_utf16().collect();
    if needle.is_empty() || units.len() < needle.len() {
        return None;
    }
    let last = units.len() - needle.len();
    if from_utf16 > last {
        return None;
    }
    return (from_utf16..=last).find(|&start| units[start..start + needle.len()] == needle[..]);
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
                context,
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
                context,
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

    /// 树拼文本的偏移换算：片段起点 + 片内偏移，选区在链接之后时从切点重新定位。
    /// 回归：Chromium 把行内链接之间的文本 run 当作段落，片段的起点在块中间，
    /// 直接拿提供方偏移会切出一句缺了开头的句子。
    #[test]
    fn assembled_offset_accounts_for_the_fragment_position() {
        let assembled = "methods (Radford et al., 2021) which require images.";
        // 片段就是块的开头：片段起点为 0，片内偏移原样可用
        assert_eq!(
            super::offset_in_assembled_text(assembled, "methods (", 0, 0, "methods"),
            0
        );
        // 片段从块中间开始（左侧被链接截断）：加片段起点
        assert_eq!(
            super::offset_in_assembled_text(assembled, " require images.", 8, 30, "images"),
            38
        );
        // 选区在片段之后：从切点重新定位
        let start = super::offset_in_assembled_text(assembled, "methods (", 9, 0, "require");
        assert_eq!(start, 37);
        assert_eq!(&assembled[start..start + "require".len()], "require");
        // 定位失败时返回推算值，由锚点校验拒掉
        assert_eq!(
            super::offset_in_assembled_text(assembled, "methods (", 9, 0, "absent"),
            9
        );
    }

    /// 片段是否从块中间开始：Chromium 的文本 run 以空白/小写/闭合标点开头说明
    /// 左边还有内容；标题、表格单元格、段落开头的片段不会满足
    #[test]
    fn fragment_start_detection() {
        assert!(super::starts_mid_block(" which require images"));
        assert!(super::starts_mid_block(") which require images"));
        assert!(super::starts_mid_block("et al. (2024)"));
        assert!(!super::starts_mid_block("Unlike weakly supervised"));
        assert!(!super::starts_mid_block("2024 results"));
    }
}
