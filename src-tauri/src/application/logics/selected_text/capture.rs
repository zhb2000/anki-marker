//! 跨平台共享的取句纯逻辑：锚点探测（校验门控）→ 跨句保留 → 单句扩展。
//!
//! 三个平台的取句链路形态一致：平台层（macOS AX / Windows UIA / Linux AT-SPI）
//! 负责取得“选中的词”与“选区周边的上下文窗口”，并以 UTF-16 码元偏移给出
//! 若干候选锚点；本模块在上下文中校验锚点并切出句子。偏移坐标系因提供方而
//! 异（详见 find_anchor），因此所有候选锚点都要经“该位置的 UTF-16 子串等于
//! 所选文本”校验后才采用——宁可降级为仅录词，也不切出错误的句子。

#[cfg(any(target_os = "macos", target_os = "windows", test))]
use super::SelectedContext;
use super::SentenceContext;
use crate::application::logics::sentence::{
    count_intersecting_sentences, find_sentence_with_range,
};
use unicode_segmentation::UnicodeSegmentation;

/// 结果日志的描述串：词与录入文本（各截断到 80 字符），用于实机定位“读错控件”
/// 类问题（日志只写本机日志文件，不上传）。
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn describe_context(context: &SelectedContext) -> String {
    fn preview(text: &str) -> String {
        const MAX_CHARS: usize = 80;
        let mut chars = text.chars();
        let preview: String = chars.by_ref().take(MAX_CHARS).collect();
        return if chars.next().is_some() {
            format!("{preview}…")
        } else {
            preview
        };
    }
    return format!(
        "word {:?}, text {:?} ({} utf16 units)",
        context.word.as_deref().map(preview),
        preview(&context.text),
        context.text.encode_utf16().count()
    );
}

/// 取句结果：单句时为用户所选词扩展出的整句；用户手动跨句选择时为其所选原文。
#[derive(Debug)]
pub enum SentenceCapture {
    /// 选区落在单个句子内，已扩展为整句（前端可预选命中的单词）。
    /// `word_offset` 为所选词在该句中的 UTF-16 起始偏移（相对 trim 后的句子）：
    /// 同一单词在句中多次出现时，前端据此标记真正被选中的那一处——按字符串
    /// 匹配只能取第一处，会标错位置。
    Expanded {
        sentence: String,
        word_offset: Option<usize>,
        /// 句子两侧的上下文（常驻显示，点击可并入）：块内已有的相邻句子
        context: SentenceContext,
    },
    /// 选区横跨多个句子：只扩展、不丢弃——原样录入用户所选文本（不做单词预选）
    SelectionAsIs(String),
}

/// 在上下文中定位选区并切句：锚点探测（校验门控）→ 跨句保留 → 单句扩展。
///
/// `anchor_candidates`：平台层给出的候选锚点（UTF-16 码元偏移），按优先级排列，
/// 经校验后取第一个命中者；全部失败时以“所选文本的唯一出现位置”兜底。
/// `window_loc`：上下文窗口起点在提供方文本中的偏移（0 表示已对齐文本开头）。
///
/// 返回切句结果，以及句子是否触及上下文边缘（触边意味着窗口可能截断了句子，
/// 调用方值得扩大窗口重试；跨句保留路径无此概念，恒为 false）。
pub fn capture_in_context(
    context: &str,
    word: &str,
    anchor_candidates: &[isize],
    window_loc: isize,
    allow_unique_occurrence_fallback: bool,
) -> Option<(SentenceCapture, bool)> {
    let sel_start_utf16 = find_anchor(
        context,
        word,
        anchor_candidates,
        allow_unique_occurrence_fallback,
    )?;
    let sel_end_utf16 = sel_start_utf16 + word.encode_utf16().count();

    // 只扩展、不丢弃：选区横跨多个句子时原样录入用户所选文本，不做分句截断。
    // 典型场景：Chrome PDF 中手动选中跨行的完整句子——PDF 视觉行间的 \n 使
    // UAX #29（SB4：CR/LF 后强制分句）把整句切为多行碎片，若仍取“与选区起点
    // 相交的句子”就只会录入第一行，丢弃了用户明确选择的内容
    if count_intersecting_sentences(context, sel_start_utf16, sel_end_utf16) > 1 {
        return Some((
            SentenceCapture::SelectionAsIs(word.trim().to_string()),
            false,
        ));
    }
    let (_, sent_start, sent_end) =
        find_sentence_with_range(context, sel_start_utf16, sel_end_utf16)?;
    // 引用标注的另一侧：UAX #29 在 “...[” 处断句，下一句会以 “30] …” 开头。
    // 标注属于上一句，把句首这段残留丢掉
    let selection_byte_start = {
        let mut units = 0usize;
        let mut byte = 0usize;
        for (offset, ch) in context.char_indices() {
            if units >= sel_start_utf16 {
                byte = offset;
                break;
            }
            units += ch.len_utf16();
            byte = offset + ch.len_utf8();
        }
        byte
    };
    let sent_start =
        skip_citation_continuation(context, sent_start, sent_end, selection_byte_start);
    // UAX #29 会在缩写处断句（"as visible in Fig. 2" → "…Fig. " + "2. "）：
    // 句尾不是真正的句末、后一段又是续接形态时并回来
    let sent_end = extend_abbreviation_tail(context, sent_start, sent_end);
    let sentence = context[sent_start..sent_end].trim().to_string();
    // 不变量守门：切出的句子必须包含所选文本——锚点经逐码元校验、句子围绕选区
    // 切出，数学上必然成立；若不成立，说明读取间隙提供方的文本模型发生了偏移
    // （动态页面）或实现存在缺陷。宁可降级为仅录词（返回 None），也不录入一句
    // 不含所选词的错句；附完整证据便于定位
    let word_trimmed = word.trim();
    if !sentence.contains(word_trimmed) {
        let excerpt_start = sel_start_utf16.saturating_sub(60);
        let excerpt: String = {
            // 按 UTF-16 偏移截取锚点周边片段（手动换算，越界夹取）
            let context_utf16: Vec<u16> = context.encode_utf16().collect();
            let start = excerpt_start.min(context_utf16.len());
            let end = (sel_end_utf16 + 60).min(context_utf16.len());
            String::from_utf16_lossy(&context_utf16[start..end])
        };
        log::warn!(
            "captured sentence does not contain the selected word, degrading to word-only \
             (word {word_trimmed:?}, sentence {sentence:?}, selection [{sel_start_utf16}, \
             {sel_end_utf16}), context {} utf16 units, excerpt around the anchor: {excerpt:?})",
            context.encode_utf16().count()
        );
        return None;
    }
    let touched_edge = (sent_start == 0 && window_loc > 0) || sent_end == context.len();
    // 所选词在句子中的 UTF-16 偏移：句子的字节区间已知，减去句子 trim 掉的前缀，
    // 再加上词自身 trim 掉的前导空白（锚点定位的是原始 word）。任一步下溢即置 None，
    // 由前端回退到按字符串匹配。
    let raw_sentence = &context[sent_start..sent_end];
    let lead_trim = raw_sentence.len() - raw_sentence.trim_start().len();
    let word_lead = word.len() - word.trim_start().len();
    let word_offset = context[..sent_start]
        .encode_utf16()
        .count()
        .checked_add(raw_sentence[..lead_trim].encode_utf16().count())
        .and_then(|sentence_start| {
            sel_start_utf16
                .checked_add(word[..word_lead].encode_utf16().count())
                .and_then(|start| start.checked_sub(sentence_start))
        });
    return Some((
        SentenceCapture::Expanded {
            sentence,
            word_offset,
            // 窗口链路没有“块”的概念，无从判断句子后面还有什么：建议只在
            // capture_in_segments（块链路）里计算
            context: Default::default(),
        },
        touched_edge,
    ));
}

/// 在上下文中定位所选文本的 UTF-16 起点（锚点探测，校验门控）。
///
/// 提供方的偏移坐标系存在已知怪癖：选区偏移与窗口文本的坐标系可能不一致
/// （macOS 上如 Obsidian 阅读模式：偏移是文档级的、返回文本却是块级的）。
/// 因此不盲信偏移，按优先级尝试候选锚点（由平台层给出，如窗口偏移 → 文档级
/// 偏移 → 0），每个候选以“该位置的 UTF-16 子串等于所选文本”校验后才采用。
///
/// `allow_unique_occurrence_fallback` 为 true 时，候选全部失败可退到“所选文本的
/// 唯一出现位置”（零次出现无法定位、多次出现有歧义，均不采用）。该兜底只适用于
/// macOS/Linux——它们的偏移由提供方直接报告、候选基本可信，兜底只是坐标系怪癖的
/// 最后机会；Windows 的偏移是本地推导的（MoveEndpointByRange 前缀法），推导一旦
/// 系统性失灵，兜底会把“定位失败”变成“自信地切出错误句子”，故传 false。
/// 仍失败返回 None——宁可降级为仅录词，也不切出错误的句子。
pub fn find_anchor(
    context: &str,
    word: &str,
    candidates: &[isize],
    allow_unique_occurrence_fallback: bool,
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
    for &candidate in candidates {
        if matches_at(candidate) {
            return Some(candidate as usize);
        }
    }
    if !allow_unique_occurrence_fallback {
        return None;
    }
    let mut occurrences = context.match_indices(word);
    return match (occurrences.next(), occurrences.next()) {
        (Some((byte_index, _)), None) => Some(context[..byte_index].encode_utf16().count()),
        _ => None,
    };
}

/// 块内的文本片段（提供方切好的段落、视觉行、文本 run）。
///
/// 这是三端共用的“块来源”抽象：平台层只负责把选区所在的块收集成若干片段、
/// 给出选区落在哪个片段及其片内偏移；段与段怎么合并、句子怎么切，都归这里。
/// 片段之间视为软换行——是否真能接回去由下面的拼行规则决定。
pub fn assemble_block_segments<S: AsRef<str>>(
    segments: &[S],
    anchor_segment: usize,
    anchor_offset_utf16: usize,
) -> (String, usize) {
    let mut block = String::new();
    let mut position = 0usize;
    let mut selection = 0usize;
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            block.push('\n');
            position += 1;
        }
        if index == anchor_segment {
            selection = position + anchor_offset_utf16;
        }
        block.push_str(segment.as_ref());
        position += segment.as_ref().encode_utf16().count();
    }
    return (block, selection);
}

/// UTF-16 码元偏移 → 字节偏移（落在代理对中间或越界时返回 None）
fn utf16_to_byte(text: &str, units: usize) -> Option<usize> {
    let mut position = 0usize;
    for (byte, ch) in text.char_indices() {
        if position == units {
            return Some(byte);
        }
        position += ch.len_utf16();
        if position > units {
            return None;
        }
    }
    return (position == units).then_some(text.len());
}

/// 去掉提供方插入的软断字符（U+0002、U+00AD），并把 UTF-16 偏移换算到新文本。
///
/// 这些字符表示“这里折行了，但不是空格”，必须先删再切句：UAX #29 会把控制字符
/// 当成句子边界，imple + U+0002 + mentation 会被切在词中间。两个字符都是单码元，
/// 偏移只需减去其前被删掉的个数。
fn strip_soft_breaks(text: &str, offset: usize) -> (String, usize) {
    let mut output = String::with_capacity(text.len());
    let mut source = 0usize;
    let mut removed = 0usize;
    for c in text.chars() {
        if matches!(c, '\u{2}' | '\u{AD}') {
            if source < offset {
                removed += 1;
            }
        } else {
            output.push(c);
        }
        source += c.len_utf16();
    }
    return (output, offset.saturating_sub(removed));
}

/// 句子在上下文中的字节区间。与 capture_in_context 的切句规则保持一致：先排除
/// “用户手选的跨句文本”，再叠加引用标注残留剔除与缩写尾补全。两处只改一处会让
/// 边界的计算与实际录入的句子错位，改动时务必对照 capture_in_context。
fn sentence_span(
    context: &str,
    sel_start_utf16: usize,
    sel_end_utf16: usize,
) -> Option<(usize, usize)> {
    if count_intersecting_sentences(context, sel_start_utf16, sel_end_utf16) > 1 {
        return None;
    }
    let (_, start, end) = find_sentence_with_range(context, sel_start_utf16, sel_end_utf16)?;
    let selection_byte = utf16_to_byte(context, sel_start_utf16)?;
    let start = skip_citation_continuation(context, start, end, selection_byte);
    let end = extend_abbreviation_tail(context, start, end);
    return Some((start, end));
}

/// 每侧最多提供几句上下文（近端优先）：给“再并入一句”留一次余量，不把整段都搬上来
const CONTEXT_SENTENCES: usize = 2;

/// 相邻句子的纯文本：去掉控制字符（PDF 的软连字符等）并裁剪首尾空白。
fn clean_context_sentence(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| *c != '\u{2}' && *c != '\u{AD}' && !(c.is_control() && !c.is_whitespace()))
        .collect();
    return cleaned.trim().to_string();
}

/// 从片段里取相邻句子：nearest_last 为 true 时取末尾几句（上文），否则取开头几句
/// （下文）。近端始终排在第一位——界面只显示第一句，并入后由下一句补位。
fn adjacent_sentences(fragment: &str, limit: usize, nearest_last: bool) -> Vec<String> {
    let mut sentences: Vec<String> = fragment
        .unicode_sentences()
        .map(clean_context_sentence)
        .filter(|text| !text.is_empty())
        .collect();
    if nearest_last {
        sentences.reverse();
    }
    sentences.truncate(limit);
    return sentences;
}

/// 由块片段直接取句：组装片段 → 按拼行规则合并 → 切句。
///
/// 返回 {取句结果, 应上报的词}——拼行会把换行折成空格，词也折成同一形态，
/// 才能通过锚点校验并与句子保持同一文本形态。
pub fn capture_in_segments<S: AsRef<str>>(
    segments: &[S],
    anchor_segment: usize,
    anchor_offset_utf16: usize,
    word: &str,
) -> Result<(SentenceCapture, String), String> {
    let (block, sel_start) = assemble_block_segments(segments, anchor_segment, anchor_offset_utf16);
    let block_word = collapse_whitespace(word);
    let sel_end = sel_start + block_word.encode_utf16().count();
    let joined_block = join_block_parts(&block, sel_start, sel_end)
        .ok_or_else(|| "the selection is not inside the block".to_string())?;
    // 提供方插在行内的结构性字符（PDF 查看器的软连字符 U+0002 等）必须在切句之前
    // 去掉：UAX #29 会把控制字符当成断句点，一句完整的话会被从词中间切开
    let (joined, joined_start) = strip_soft_breaks(&joined_block.text, joined_block.selection);
    // 偏移是本地推导的，与 Windows 同理不用“唯一出现位置”兜底（见 find_anchor）；
    // window_loc 传 0：上下文就是块本身，句子贴着块的起点/终点不是“被窗口截断”
    // 提供方报的选区偶尔差一个码元（Chromium 在行内节点边界上实测会偏 1），
    // 候选偏移带上 ±1：仍要逐码元校验等于词，不会因此切出别的句子
    let candidates = [
        joined_start as isize,
        joined_start as isize - 1,
        joined_start as isize + 1,
    ];
    // Chromium 的自动断行会在视觉折行处插入 U+2010，而 AX 树里的原文没有这个字符：
    // 选中 "em‐bedding" 时词里带连字符，块文本里是 "embeddings"，校验会对不上。
    // 原文匹配不到时，用去掉折行连字符的形态再试一次
    for candidate in word_variants(&block_word) {
        if let Some((mut capture, _)) =
            capture_in_context(&joined, &candidate, &candidates, 0, false)
        {
            // 句子两侧的上下文：块内已有的相邻句子，**常驻**（不看“是否可疑”）。
            // 需要补救的时刻恰恰是算法以为自己取对了的时候，条件显示覆盖不到
            if let SentenceCapture::Expanded { context, .. } = &mut capture {
                if let Some((start, end)) = sentence_span(&joined, joined_start, sel_end) {
                    let tail = joined_block.tail.trim();
                    let extended = if tail.is_empty() {
                        joined.clone()
                    } else {
                        format!("{joined} {tail}")
                    };
                    context.before = adjacent_sentences(&joined[..start], CONTEXT_SENTENCES, true);
                    context.after = adjacent_sentences(&extended[end..], CONTEXT_SENTENCES, false);
                }
            }
            return Ok((capture, complete_word_at(&joined, joined_start, &candidate)));
        }
    }
    return Err(anchor_error(
        &joined,
        &block_word,
        joined_start,
        joined.encode_utf16().count(),
    ));
}

/// 锚点校验失败时的诊断串：带坐标系关键参数，便于实机定位（日志只写本机文件）。
pub fn anchor_error(context: &str, word: &str, sel_start: usize, context_units: usize) -> String {
    return format!(
        "no validated anchor for the selected text in the context \
         (selection starts at {sel_start} utf16 units, context {context_units} utf16 units, \
         word {} utf16 units, {} occurrence(s) in the context; \
         a selection spanning several paragraphs lands here too)",
        word.encode_utf16().count(),
        context.match_indices(word).count()
    );
}

/// 折叠连续空白为单个空格（行首行尾空白一并收拾）。
pub fn collapse_whitespace(text: &str) -> String {
    return text.split_whitespace().collect::<Vec<_>>().join(" ");
}

/// 段落文本是否在中间被切断：提供方会在内嵌对象（公式、图片）前停下（"…states "），
/// 或在引用标注这类文本片段边界上停下（"…memory ["，PDF 里 "[13]" 常自成一段）。
/// 两种情况都不可能是完整的块，需要继续拼行；标题、表格单元格两种形态都不满足。
pub fn block_looks_cut(text: &str) -> bool {
    return text.ends_with(char::is_whitespace) || block_ends_with_open_marker(text);
}

/// 段落以开括号/连字符结尾：不可能是一个完整的块。网页的行内引用链接、PDF 的引用
/// 标注都常以 "(" 或 "[" 开头，段落文本会在那里断掉。
pub fn block_ends_with_open_marker(text: &str) -> bool {
    return matches!(
        text.chars().next_back(),
        Some('[' | '(' | '{' | '“' | '‘' | '-' | '\u{2010}')
    );
}

/// 词的可能形态：原文，以及去掉折行连字符（U+2010 / U+2011 / 软连字符）后的形态。
/// 视觉折行处的连字符由浏览器排版插入，AX 树里的原文并没有它。
fn word_variants(word: &str) -> Vec<String> {
    let cleaned: String = word
        .chars()
        .filter(|c| !matches!(c, '\u{2010}' | '\u{2011}' | '\u{00AD}'))
        .collect();
    if cleaned == word || cleaned.is_empty() {
        return vec![word.to_string()];
    }
    return vec![word.to_string(), cleaned];
}

/// 把词补到完整单词边界：提供方偶尔少报选区末尾的字符（Chromium 在行内节点边界
/// 实测把 "Examples" 报成 "Example"），偏移不变，只把预选词补全，避免词典查错词。
fn complete_word_at(text: &str, start_utf16: usize, word: &str) -> String {
    let mut utf16 = 0usize;
    let mut start_byte = None;
    for (byte, ch) in text.char_indices() {
        if utf16 >= start_utf16 {
            start_byte = Some(byte);
            break;
        }
        utf16 += ch.len_utf16();
    }
    let Some(start_byte) = start_byte else {
        return word.to_string();
    };
    let rest = &text[start_byte..];
    if !rest.starts_with(word) {
        return word.to_string();
    }
    let mut end = start_byte + word.len();
    while let Some(ch) = text[end..].chars().next() {
        if !ch.is_alphanumeric() {
            break;
        }
        end += ch.len_utf8();
    }
    return text[start_byte..end].to_string();
}

/// 缩写处的伪句末（"Fig. 2"、"Eq. 3"、"et al. 2020"）：分句在缩写后断开，后一段
/// 若以数字/小写/续接标点开头，就把它并回本句，返回新的句子结束字节偏移。
///
/// unicode_sentences() 会过滤掉不含字母数字的句段，切片不连续，偏移一律按指针换算，
/// 并允许两段之间只隔空白。
fn extend_abbreviation_tail(context: &str, start: usize, end: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;

    let base = context.as_ptr() as usize;
    let mut end = end;
    // 连续多个缩写也不多见，限制次数防御死循环
    for _ in 0..4 {
        let current = &context[start..end];
        if ends_sentence(current.trim_end()) {
            return end;
        }
        let rest = &context[end..];
        // 引用标注被 UAX #29 当成新句起点（“...conserve 1758.[” + “23][24] In 2007”）。
        // 句号在标注之前就结束了句子，标注属于上一句：并回标注并在此收句
        if current.ends_with('[') {
            if let Some(length) = citation_marker_length(rest) {
                return end + length;
            }
        }
        let Some(next) = rest.unicode_sentences().next() else {
            return end;
        };
        let next_start = next.as_ptr() as usize - base;
        let next_end = next_start + next.len();
        if !context[end..next_start].trim().is_empty() {
            return end;
        }
        if !can_join_lines(current, next) {
            return end;
        }
        end = next_end;
    }
    return end;
}

/// 句首的引用标注残留（`30] The genus…`）属于上一句：跳过 `NN]` 这类碎片。
fn skip_citation_continuation(
    context: &str,
    mut start: usize,
    end: usize,
    selection_start: usize,
) -> usize {
    loop {
        let slice = &context[start..end];
        let trimmed = slice.trim_start();
        let lead = slice.len() - trimmed.len();
        let Some(consumed) = leading_citation_length(trimmed) else {
            return start;
        };
        let next = start + lead + consumed;
        // 选区落在标注碎片里时不跳过，否则句子会不含所选文本
        if next > selection_start {
            return start;
        }
        start = next;
    }
}

/// 句首引用标注的长度：`[30]`、`[30][31]`，或断句后残留的 `30]`。
fn leading_citation_length(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.first() == Some(&b'[') {
        let mut index = 0usize;
        let mut consumed = None;
        while bytes.get(index) == Some(&b'[') {
            let mut cursor = index + 1;
            let mut saw_alnum = false;
            while cursor < bytes.len() && bytes[cursor] != b']' {
                let byte = bytes[cursor];
                if byte.is_ascii_alphanumeric() {
                    saw_alnum = true;
                } else if !matches!(byte, b' ' | b',' | b';' | b'-') {
                    return consumed;
                }
                cursor += 1;
            }
            if cursor >= bytes.len() || !saw_alnum {
                return consumed;
            }
            index = cursor + 1;
            consumed = Some(index);
        }
        return consumed;
    }
    // “30] …”：开括号已经在上一段末尾
    let mut digits = 0usize;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    return (digits > 0 && bytes.get(digits) == Some(&b']')).then_some(digits + 1);
}

/// `rest` 以引用标注的剩余部分开头（`23][24]…`）时，返回标注结束处的字节长度。
///
/// UAX #29 会在 “...1758.[” 处断句（句号后紧跟开括号），标注的余下部分被当成新句
/// 的开头；标注属于上一句，必须并回去，否则会把后面整段正文一起并进来。
fn citation_marker_length(rest: &str) -> Option<usize> {
    // 标注内容都是 ASCII（数字、字母、逗号、分隔符）；遇到其他字节就放弃，
    // 交给原有的拼行逻辑处理
    let bytes = rest.as_bytes();
    let mut index = 0usize;
    let mut saw_alnum = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b']' {
            if !saw_alnum {
                return None;
            }
            index += 1;
            // 连续标注：“[24][25]”
            if bytes.get(index) == Some(&b'[') {
                index += 1;
                saw_alnum = false;
                continue;
            }
            return Some(index);
        }
        if byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b',' | b';' | b'-') {
            saw_alnum |= byte.is_ascii_alphanumeric();
            index += 1;
            continue;
        }
        return None;
    }
    return None;
}

/// 把以换行分隔的上下文按“块”重建：从选区所在行出发逐行外扩，只在允许连接处合并。
///
/// 行级提供方（Chromium 的 PDF 查看器等）把文本按视觉行吐出，行间是换行；UAX #29
/// 在换行处强制分句（SB4），直接分句只能得到一行。但也不能像早期实现那样先把所有
/// 换行抹成空格再分句——那会把不带句末标点的块（标题、表格单元格、列表项）与相邻
/// 块粘成一句。这里改成“块内拼行、块间不拼”：
/// - 外扩时只有上一行未结束句子、且下一行以续接形态开头才合并；
/// - 行尾软连字符（per- + centage）合并时去掉连字符；
/// - 空行不算块边界：Chromium 的 PDF 查看器用连续换行分隔视觉行，同一窗口里空行
///   并不代表段落（实测两种形态都出现），块边界交给文本规则判断。
///
/// 返回合并后的上下文与选区起点在新上下文中的 UTF-16 偏移；选区定位失败返回 None
/// （调用方降级，不猜位置）。
/// 选区所在块，以及同一收集窗口里块之后的文本。
pub struct JoinedBlock {
    /// 合并后的块文本（选区的句子从这里切）
    pub text: String,
    /// 选区起点在 `text` 中的 UTF-16 偏移
    pub selection: usize,
    /// 块之后的后续行（trim 后以空格连接）：并入建议的材料
    pub tail: String,
}

/// 只要合并结果、不关心块后文本的便捷入口（单测与早期调用方使用）
#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(test), allow(dead_code))]
pub fn join_block_around_anchor(
    raw: &str,
    sel_start_utf16: usize,
    sel_end_utf16: usize,
) -> Option<(String, usize)> {
    let joined = join_block_parts(raw, sel_start_utf16, sel_end_utf16)?;
    return Some((joined.text, joined.selection));
}

#[cfg(any(target_os = "macos", test))]
pub fn join_block_parts(
    raw: &str,
    sel_start_utf16: usize,
    sel_end_utf16: usize,
) -> Option<JoinedBlock> {
    let lines = split_lines_utf16(raw);
    // 空行不参与拼接判定：Chromium 的 PDF 查看器用连续换行分隔视觉行，空行不代表段落
    let usable: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, (_, text))| !text.trim().is_empty())
        .map(|(index, _)| index)
        .collect();
    if usable.is_empty() {
        return None;
    }
    // 选区起点所在行（起点落在行尾、等于下一行起点时归下一行；越界时取最后一行）
    let anchor_pos = usable
        .iter()
        .position(|index| {
            let (start, text) = &lines[*index];
            let end = start + text.encode_utf16().count();
            *start <= sel_start_utf16 && sel_start_utf16 < end
        })
        .unwrap_or(usable.len() - 1);
    // 选区终点所在行：手动跨行选择时整段保留，不受连接规则约束
    let anchor_end_pos = usable
        .iter()
        .enumerate()
        .skip(anchor_pos)
        .take_while(|(_, index)| lines[**index].0 < sel_end_utf16)
        .map(|(position, _)| position)
        .last()
        .unwrap_or(anchor_pos);

    let mut first_pos = anchor_pos;
    while first_pos > 0
        && can_join_lines(lines[usable[first_pos - 1]].1, lines[usable[first_pos]].1)
    {
        first_pos -= 1;
    }
    let mut last_pos = anchor_end_pos;
    while last_pos + 1 < usable.len()
        && can_join_lines(lines[usable[last_pos]].1, lines[usable[last_pos + 1]].1)
    {
        last_pos += 1;
    }
    let first = usable[first_pos];
    let last = usable[last_pos];

    let mut output = String::new();
    let mut output_utf16 = 0usize;
    let mut output_sel: Option<usize> = None;
    for (index, (start, raw_line)) in lines.iter().enumerate() {
        if index < first || index > last {
            continue;
        }
        let text = raw_line.trim();
        if text.is_empty() {
            continue;
        }
        if !output.is_empty() {
            let previous = output.chars().next_back();
            let next = text.chars().next();
            let hyphenated =
                matches!(previous, Some('-' | '\u{2010}')) && next.is_some_and(char::is_lowercase);
            if previous == Some('\u{2}') {
                // 提供方的行尾软连字符：去掉它直接相接（lan + guage → language），
                // 既不补连字符也不插空格
                output.pop();
                output_utf16 -= 1;
                if let Some(offset) = output_sel {
                    if offset > output_utf16 {
                        output_sel = Some(offset - 1);
                    }
                }
            } else if hyphenated {
                // 行尾软连字符：去掉连字符直接相接（per- + centage → percentage）
                output.pop();
                output_utf16 -= 1;
                if let Some(offset) = output_sel {
                    if offset > output_utf16 {
                        output_sel = Some(offset - 1);
                    }
                }
            } else if needs_space(previous, next) {
                output.push(' ');
                output_utf16 += 1;
            }
        }
        if output_sel.is_none() {
            let line_utf16 = raw_line.encode_utf16().count();
            if *start <= sel_start_utf16 && sel_start_utf16 <= *start + line_utf16 {
                let lead_trim = raw_line.len() - raw_line.trim_start().len();
                let lead_utf16 = raw_line[..lead_trim].encode_utf16().count();
                output_sel = Some(
                    output_utf16
                        + sel_start_utf16
                            .saturating_sub(*start)
                            .saturating_sub(lead_utf16),
                );
            }
        }
        output.push_str(text);
        output_utf16 += text.encode_utf16().count();
    }
    // 块之后的行：并入建议的材料。只取块以外的行，块内的句子不参与
    let tail = lines
        .iter()
        .enumerate()
        .filter(|(index, _)| *index > last)
        .map(|(_, (_, text))| text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    return Some(JoinedBlock {
        text: output,
        selection: output_sel?,
        tail,
    });
}

/// 上下文的行序列：每行给出它在原文中的 UTF-16 起点与内容（不含换行符）。
#[cfg(any(target_os = "macos", test))]
fn split_lines_utf16(raw: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut start_byte = 0usize;
    let mut start_utf16 = 0usize;
    let mut utf16 = 0usize;
    for (byte, ch) in raw.char_indices() {
        if ch == '\n' {
            lines.push((start_utf16, &raw[start_byte..byte]));
            start_byte = byte + 1;
            start_utf16 = utf16 + 1;
        }
        utf16 += ch.len_utf16();
    }
    lines.push((start_utf16, &raw[start_byte..]));
    return lines;
}

/// 两行能否合并成同一块：上一行未结束句子，且下一行是续接形态。
///
/// 这是“标题/表格单元格不被并进句子”的核心判据：标题与单元格都不带句末标点，
/// 但它们的下一行以大写字母开头（新块），因此不会被合并。
#[cfg(any(target_os = "macos", test))]
pub fn can_join_lines(previous: &str, next: &str) -> bool {
    let previous = previous.trim_end();
    let next = next.trim_start();
    if previous.is_empty() || next.is_empty() {
        return false;
    }
    // 行尾是提供方的软连字符（PDF 查看器把行尾连字符输出为 U+0002）：词被换行切成
    // 两半，必须接上，与下一行的首字符无关
    if previous.ends_with('\u{2}') {
        return true;
    }
    if ends_sentence(previous) {
        return false;
    }
    // 行尾停在分号/逗号/开括号这类“显然没说完”的位置：本身就是续接证据。
    // 引用串常在这里断开，下一行以作者名（大写）开头——只看首字母就会漏掉
    if ends_with_continuation_marker(previous) {
        return true;
    }
    return starts_continuation(next);
}

/// 行尾是否停在“不可能是一个块的结尾”的标点上。
///
/// 只看右侧首字母的续接判定在这里失效：引用串 “…(Mikolov et al., 2013;” 的下一行是
/// 另一条引用 “Bojanowski et al., 2017)”，首字母大写，规则会判成“新块”，句子就被
/// 切在第一个引用处。这些标点本身已经说明话没说完，不必再看下一行。
fn ends_with_continuation_marker(text: &str) -> bool {
    return matches!(
        text.chars().next_back(),
        Some(';' | ',' | '(' | '[' | '{' | '–' | '—' | '/' | '&')
    );
}

/// 行尾是否结束了一个句子：去掉尾随引号/括号后以句末标点结尾。
/// 缩写（Fig. / Eq. / et al. / 单字母缩写等）不算句末——论文里极常见。
#[cfg(any(target_os = "macos", test))]
fn ends_sentence(text: &str) -> bool {
    let trimmed = text.trim_end_matches(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '”' | '’' | ')' | ']' | '»' | '）' | '】' | '》'
            )
    });
    let Some(last) = trimmed.chars().next_back() else {
        return false;
    };
    if !matches!(last, '.' | '!' | '?' | '…' | '。' | '！' | '？') {
        return false;
    }
    if last == '.' && ends_with_abbreviation(trimmed) {
        return false;
    }
    return true;
}

/// 行尾的点号是缩写还是句号：点号前的“词”是 1 到 3 个字母（Fig./Eq./al./U.S.）时
/// 按缩写处理，允许继续拼行。
#[cfg(any(target_os = "macos", test))]
fn ends_with_abbreviation(text: &str) -> bool {
    let head = text.trim_end_matches('.').trim_end();
    let token: Vec<char> = head
        .chars()
        .rev()
        .take_while(|c| c.is_alphabetic())
        .collect();
    return !token.is_empty() && token.len() <= 3;
}

/// 下一行是否为续接形态：除大写字母开头（新块：标题、新句子）外都算续接。
///
/// 这条规则要覆盖行级提供方的各种切法：小写单词、数字、CJK、以及被公式/标点
/// 切开的碎片（"," "−" "1" "."）。判断“上一行是否结束句子”已经挡掉了真正的
/// 句子边界，剩下的非大写开头都应当接回去。
#[cfg(any(target_os = "macos", test))]
fn starts_continuation(text: &str) -> bool {
    let Some(first) = text.chars().next() else {
        return false;
    };
    return !first.is_uppercase();
}

/// CJK 字符：判断拼行要不要补空格，以及“无大小写”的续接行。
#[cfg(any(target_os = "macos", test))]
fn is_cjk(c: char) -> bool {
    return matches!(
        c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF
    );
}

/// 拼行时两行之间要不要插空格：闭合标点前不插、开括号后不插、CJK 之间不插，
/// 其余插一个空格。
#[cfg(any(target_os = "macos", test))]
fn needs_space(previous: Option<char>, next: Option<char>) -> bool {
    let (Some(previous), Some(next)) = (previous, next) else {
        return false;
    };
    if matches!(
        next,
        '.' | ','
            | ';'
            | ':'
            | '!'
            | '?'
            | '…'
            | ')'
            | ']'
            | '}'
            | '、'
            | '，'
            | '；'
            | '：'
            | '。'
            | '！'
            | '？'
            | '）'
            | '】'
            | '」'
            | '』'
            | '”'
            | '’'
    ) {
        return false;
    }
    if matches!(previous, '(' | '[' | '{' | '“' | '‘') {
        return false;
    }
    if is_cjk(previous) && is_cjk(next) {
        return false;
    }
    return true;
}

/// 将字符（Unicode codepoint）偏移换算为 UTF-16 码元偏移。
///
/// AT-SPI 的文本偏移按字符计，而共享分句逻辑（sentence 模块）以 UTF-16 码元计；
/// astral 字符（如 emoji）二者不同。偏移越界返回 None（调用方降级处理）。
/// 仅 Linux（AT-SPI）使用；test 保留以便单测在各平台运行。
#[cfg(any(target_os = "linux", test))]
pub fn char_offset_to_utf16(s: &str, char_offset: usize) -> Option<usize> {
    let mut utf16_offset = 0;
    for (index, ch) in s.chars().enumerate() {
        if index == char_offset {
            return Some(utf16_offset);
        }
        utf16_offset += ch.len_utf16();
    }
    // char_offset == 字符总数（指向文本末尾）也是合法偏移
    return (char_offset == s.chars().count()).then_some(utf16_offset);
}

/// AT-SPI 超文本模型里内嵌子对象（链接、行内 code、图片等）在父对象 Text 中的占位符。
/// 仅 Linux（AT-SPI）使用；test 保留以便单测在各平台运行。
#[cfg(any(target_os = "linux", test))]
pub const EMBEDDED_OBJECT_CHAR: char = '\u{FFFC}';

/// 一个内嵌子对象的展开结果。
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone)]
pub struct EmbeddedObject {
    /// 子对象的文本（图片等非文本对象为空串）
    pub text: String,
    /// 选区落在子对象内的部分（相对 text 的字符偏移，[start, end)）。
    /// None 表示子对象整体算作选中：选区在父对象里只能精确到占位符这一个字符，
    /// 只选了链接文字的一部分时，要靠它还原真实的选区边界。
    pub selected: Option<(usize, usize)>,
}

/// 含内嵌对象的取句结果。
#[cfg(any(target_os = "linux", test))]
#[derive(Debug)]
pub struct EmbeddedCapture {
    /// 真实的所选文本：词里的占位符已展开
    pub word: String,
    pub capture: SentenceCapture,
    /// 句子是否触及上下文边缘，语义同 capture_in_context
    pub touched_edge: bool,
}

/// 占位符展开后的上下文与选区（UTF-16 码元偏移，[start, end)）。
#[cfg(any(target_os = "linux", test))]
struct ExpandedSelection {
    context: String,
    sel_start_utf16: usize,
    sel_end_utf16: usize,
}

/// 把上下文里的占位符逐个替换为子对象文本，并同步换算选区。
///
/// `objects` 与上下文里的占位符按出现顺序一一对应，None 表示未能解析：该占位符
/// 原样保留，由调用方据此降级。数量不足、选区越界时返回 None。
#[cfg(any(target_os = "linux", test))]
fn expand_embedded_objects(
    context: &str,
    sel_start_chars: usize,
    sel_end_chars: usize,
    objects: &[Option<EmbeddedObject>],
) -> Option<ExpandedSelection> {
    let mut expanded = String::with_capacity(context.len());
    let mut utf16_len = 0usize;
    let mut sel_start_utf16 = None;
    let mut sel_end_utf16 = None;
    let mut next_object = 0usize;
    let mut char_buffer = [0u8; 4];
    for (index, ch) in context.chars().enumerate() {
        let object = if ch == EMBEDDED_OBJECT_CHAR {
            let object = objects.get(next_object)?.as_ref();
            next_object += 1;
            object
        } else {
            None
        };
        let piece: &str = match object {
            Some(object) => &object.text,
            None => ch.encode_utf8(&mut char_buffer),
        };
        let piece_utf16 = piece.encode_utf16().count();
        // 选区端点落在子对象上时，按子对象内的选区收窄
        let (inner_start, inner_end) = match object.and_then(|object| object.selected) {
            Some((start, end)) if start <= end => (
                char_offset_to_utf16(piece, start)?,
                char_offset_to_utf16(piece, end)?,
            ),
            Some(_) => return None,
            None => (0, piece_utf16),
        };
        if index == sel_start_chars {
            sel_start_utf16 = Some(utf16_len + inner_start);
        }
        if index + 1 == sel_end_chars {
            sel_end_utf16 = Some(utf16_len + inner_end);
        }
        expanded.push_str(piece);
        utf16_len += piece_utf16;
    }
    return Some(ExpandedSelection {
        context: expanded,
        sel_start_utf16: sel_start_utf16?,
        sel_end_utf16: sel_end_utf16?,
    });
}

/// 在含内嵌对象的上下文中切句：先展开占位符并换算选区，再走 capture_in_context。
///
/// `word` 与 `context` 都是 AT-SPI 读到的原文（可含占位符），`sel_start_chars` 是
/// `word` 在 `context` 中的字符偏移。未解析的占位符只要落进词或句子就返回 None：
/// 占位符背后往往是链接文字，去掉它等于录入缺字的句子——宁可降级为仅录词。
#[cfg(any(target_os = "linux", test))]
pub fn capture_with_embedded_objects(
    context: &str,
    word: &str,
    sel_start_chars: usize,
    objects: &[Option<EmbeddedObject>],
    window_loc: isize,
) -> Option<EmbeddedCapture> {
    let word_chars = word.chars().count();
    let expanded = expand_embedded_objects(
        context,
        sel_start_chars,
        sel_start_chars + word_chars,
        objects,
    )?;
    let word_has_object = word.contains(EMBEDDED_OBJECT_CHAR);
    let real_word = if word_has_object {
        // 词要从展开后的选区还原，偏移必须可信：原文切片须与读到的词逐字符一致
        if !context
            .chars()
            .skip(sel_start_chars)
            .take(word_chars)
            .eq(word.chars())
        {
            return None;
        }
        let context_utf16: Vec<u16> = expanded.context.encode_utf16().collect();
        let range = expanded.sel_start_utf16..expanded.sel_end_utf16;
        let real_word = String::from_utf16(context_utf16.get(range)?).ok()?;
        if real_word.trim().is_empty() || real_word.contains(EMBEDDED_OBJECT_CHAR) {
            return None;
        }
        real_word
    } else {
        word.to_string()
    };
    // 词是由偏移还原出来的时候，唯一出现兜底没有校验意义，只有原文词才允许
    let (capture, touched_edge) = capture_in_context(
        &expanded.context,
        &real_word,
        &[expanded.sel_start_utf16 as isize],
        window_loc,
        !word_has_object,
    )?;
    if let SentenceCapture::Expanded { sentence, .. } = &capture {
        if sentence.contains(EMBEDDED_OBJECT_CHAR) {
            return None;
        }
    }
    return Some(EmbeddedCapture {
        word: real_word,
        capture,
        touched_edge,
    });
}

/// 记录本次读到的 PRIMARY 内容，返回它相对上次记录是否变化（首次读取视为变化）。
///
/// X11 下取消选中后 PRIMARY 往往不会清空，单看内容分不清“新选的”和“很久以前选的”，
/// 只能靠与上次比较判断。仅 Linux 使用；test 保留以便单测在各平台运行。
#[cfg(any(target_os = "linux", test))]
pub fn remember_primary(last: &mut Option<String>, current: &str) -> bool {
    if last.as_deref() == Some(current) {
        return false;
    }
    *last = Some(current.to_string());
    return true;
}

/// 用权威选区文本调和平台 API 的捕获结果。
///
/// 权威文本来自模拟复制读到的剪贴板内容——拷贝成功的语义下它恒为用户真实选区；
/// 而平台无障碍 API 报告的选区可能漂移（Edge PDF 的 Adobe 引擎：文本层与视觉渲染
/// 错位，实测选区偏移 2 个字符，"reconstruction" 读出为 "onstruction qu"）或在
/// 极端情况下撞错控件。
///
/// 调和规则（`primary` 为 None/空表示无法取得权威文本，原样返回平台结果）：
/// - 平台取句成功（有 word）：句子包含权威词 → 句子保留、词替换为权威词（修正
///   漂移，前端预选与词典查询随之恢复正常）；不包含 → 平台读错了选区，降级为仅
///   录入权威文本；
/// - 平台未取句（降级/原样录入）：直接以权威文本为准。
///
/// 仅 Windows 使用（macOS AX 的选区报告未见漂移，保持纯 AX 链路）；test 保留以便
/// 单测在各平台运行。
#[cfg(any(target_os = "windows", test))]
pub fn reconcile_with_primary(
    text: String,
    word: Option<String>,
    word_offset: Option<usize>,
    primary: Option<String>,
) -> SelectedContext {
    let Some(primary) = primary
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
    else {
        return SelectedContext {
            context: Default::default(),
            text,
            word,
            word_offset,
        };
    };
    return match word {
        Some(platform_word) if text.contains(&primary) => {
            let replaced = platform_word.trim() != primary;
            if replaced {
                // 词漂移但句子正确：替换为权威词（Edge PDF 文本层错位场景）
                log::info!(
                    "capture reconciled with the clipboard: word {:?} -> {:?}",
                    platform_word,
                    primary
                );
            }
            SelectedContext {
                context: Default::default(),
                text,
                word: Some(primary),
                // 词被替换时原偏移指向的是漂移前的词，作废；词未变则偏移继续有效
                word_offset: if replaced { None } else { word_offset },
            }
        }
        Some(platform_word) => {
            // 句子不含权威词：平台读错了选区（撞错控件/漂移越界），降级录权威文本
            log::warn!(
                "platform capture does not match the clipboard selection, degrading to the \
                 clipboard text (platform word {platform_word:?}, platform text {:.80?}, \
                 clipboard {primary:?})",
                text
            );
            SelectedContext {
                context: Default::default(),
                text: primary,
                word: None,
                word_offset: None,
            }
        }
        None => SelectedContext {
            context: Default::default(),
            text: primary,
            word: None,
            word_offset: None,
        },
    };
}

#[cfg(test)]
mod tests {
    use super::{
        assemble_block_segments, block_looks_cut, capture_in_context, capture_in_segments,
        capture_with_embedded_objects, char_offset_to_utf16, expand_embedded_objects, find_anchor,
        join_block_around_anchor, reconcile_with_primary, remember_primary, EmbeddedCapture,
        EmbeddedObject, SentenceCapture, EMBEDDED_OBJECT_CHAR,
    };

    /// 引用标注的另一侧：下一句以 "30] …" 开头时，标注碎片属于上一句，要丢掉
    #[test]
    fn citation_fragment_is_dropped_from_the_sentence_start() {
        let context = "…ago.[29] [30] The genus Felis diverged around 6 million years ago.";
        let word = "Felis";
        let sel_start = context[..context.find(word).unwrap()]
            .encode_utf16()
            .count() as isize;
        let (capture, _) = capture_in_context(context, word, &[sel_start], 0, false).unwrap();
        match capture {
            SentenceCapture::Expanded { sentence, .. } => {
                assert_eq!(
                    sentence,
                    "The genus Felis diverged around 6 million years ago."
                );
            }
            SentenceCapture::SelectionAsIs(text) => panic!("unexpected selection: {text:?}"),
        }
    }

    /// 引用标注被 UAX #29 当成新句起点（"conserve 1758.[" + "23][24] In 2007"）：
    /// 标注属于上一句，必须并回去，否则会把后面整段正文一起并进来
    #[test]
    fn citation_markers_stay_with_the_sentence() {
        let context =
            "In 2003, the name was conserved.[23][24] In 2007, the modern cat was sampled.";
        let word = "name";
        let sel_start = context[..context.find(word).unwrap()]
            .encode_utf16()
            .count() as isize;
        let (capture, _) = capture_in_context(context, word, &[sel_start], 0, false).unwrap();
        match capture {
            SentenceCapture::Expanded { sentence, .. } => {
                assert_eq!(sentence, "In 2003, the name was conserved.[23][24]");
            }
            SentenceCapture::SelectionAsIs(text) => panic!("unexpected selection: {text:?}"),
        }
    }

    /// 锚点候选 1：提供方忠实响应请求窗口（窗口偏移有效）
    #[test]
    fn anchor_window_offset() {
        // 文档 "Hello world. This is a test."，窗口 loc=9，word "This" 在文档偏移 13
        let context = "ld. This is a te";
        // 候选 1：13 - 9 = 4，context[4..8] == "This"
        assert_eq!(
            find_anchor(context, "This", &[13 - 9, 13, 0], true),
            Some(4)
        );
    }

    /// 锚点候选 2：提供方忽略窗口起点、从文档开头返回文本（文档级偏移）
    #[test]
    fn anchor_document_offset() {
        let context = "Hello world. This is";
        // 候选 1（13-9=4）命中 "o wo" 不匹配；候选 2（13）命中 "This"
        assert_eq!(
            find_anchor(context, "This", &[13 - 9, 13, 0], true),
            Some(13)
        );
    }

    /// 锚点候选 3：提供方从选区开始返回文本
    #[test]
    fn anchor_selection_start() {
        let context = "This is a test.";
        // 候选 1（4）与候选 2（13，越界）均失败；候选 3（0）命中
        assert_eq!(
            find_anchor(context, "This", &[13 - 9, 13, 0], true),
            Some(0)
        );
    }

    /// 锚点兜底：偏移完全失真时靠唯一出现位置定位；多处出现有歧义返回 None
    #[test]
    fn anchor_unique_occurrence() {
        let context = "jumped over. The fox runs. End.";
        assert_eq!(
            find_anchor(context, "fox", &[9999 - 9000, 9999, 0], true),
            Some("jumped over. The ".encode_utf16().count())
        );
        // 两次出现 → 歧义，不采用（上下文开头放非匹配内容，避免候选 0 先命中）
        assert_eq!(
            find_anchor("a fox and fox", "fox", &[999, 9999, 0], true),
            None
        );
    }

    /// 所选文本不在上下文中：返回 None（降级为仅录词，不切错句）
    #[test]
    fn anchor_not_found() {
        assert_eq!(find_anchor("Hello world.", "zebra", &[0], true), None);
    }

    /// 含 emoji（UTF-16 代理对）的所选文本：按 UTF-16 码元校验与定位
    #[test]
    fn anchor_with_emoji() {
        // "say " 共 4 个 UTF-16 码元，😀 占 2 个
        assert_eq!(find_anchor("say 😀 hi.", "😀", &[4], true), Some(4));
    }

    /// 禁用唯一出现兜底（Windows 语义：推导偏移校验失败即放弃，不得猜位置）：
    /// 词在上下文中唯一出现、但候选失真时，允许兜底命中、禁用则返回 None
    #[test]
    fn anchor_unique_occurrence_fallback_disabled() {
        let context = "jumped over. The fox runs. End.";
        let expected = "jumped over. The ".encode_utf16().count();
        assert_eq!(find_anchor(context, "fox", &[999], true), Some(expected));
        assert_eq!(find_anchor(context, "fox", &[999], false), None);
    }

    /// 块级上下文取句（Obsidian 阅读模式场景）：偏移是文档级的、
    /// 上下文窗口只返回段落块，靠锚点探测取到块内完整句子
    #[test]
    fn capture_in_block_context() {
        let context = "First sentence here. The target word is fox.";
        let (capture, touched_edge) =
            capture_in_context(context, "fox", &[5000 - 4000, 5000, 0], 4000, true)
                .expect("should capture");
        match capture {
            SentenceCapture::Expanded {
                sentence,
                word_offset,
                ..
            } => {
                assert_eq!(sentence, "The target word is fox.");
                // "fox" 在句中的 UTF-16 偏移（"The target word is " 共 19 个码元）
                assert_eq!(word_offset, Some(19));
            }
            _ => panic!("expected Expanded"),
        }
        // 句子终点==上下文终点 → 触边（调用方会扩大窗口重试一次）
        assert!(touched_edge);
    }

    /// 偏移区分句中重复出现的同一单词：选中第二次出现时偏移指向第二次（字符串匹配只能取第一处）
    #[test]
    fn word_offset_picks_the_selected_occurrence() {
        let context = "The encoder runs. The encoder stops.";
        let second = context.rfind("encoder").expect("two occurrences");
        let sel_utf16 = context[..second].encode_utf16().count();
        let (capture, _) = capture_in_context(context, "encoder", &[sel_utf16 as isize], 0, false)
            .expect("should capture");
        match capture {
            SentenceCapture::Expanded {
                sentence,
                word_offset,
                ..
            } => {
                assert_eq!(sentence, "The encoder stops.");
                assert_eq!(word_offset, Some(4));
            }
            _ => panic!("expected Expanded"),
        }
    }

    /// 偏移以 trim 后的句子为基准：句子带前导空白时仍指向词的实际位置
    #[test]
    fn word_offset_survives_trim() {
        let context = "  Hello world.  ";
        let (capture, _) =
            capture_in_context(context, "world", &[8], 0, true).expect("should capture");
        match capture {
            SentenceCapture::Expanded {
                sentence,
                word_offset,
                ..
            } => {
                assert_eq!(sentence, "Hello world.");
                assert_eq!(word_offset, Some(6));
            }
            _ => panic!("expected Expanded"),
        }
    }

    /// 跨句选择保留原文（只扩展、不丢弃），且不触发触边重试
    #[test]
    fn capture_selection_as_is() {
        let context = "One. Two three. Four.";
        let word = "Two three. Four"; // 用户手动跨句选择
        let (capture, touched_edge) =
            capture_in_context(context, word, &[5], 0, true).expect("should capture");
        match capture {
            SentenceCapture::SelectionAsIs(text) => assert_eq!(text, word),
            _ => panic!("expected SelectionAsIs"),
        }
        assert!(!touched_edge);
    }

    /// 字符偏移 → UTF-16 偏移换算（AT-SPI 用）：ASCII、CJK、astral 字符、边界
    #[test]
    fn char_to_utf16_offset() {
        assert_eq!(char_offset_to_utf16("hello", 0), Some(0));
        assert_eq!(char_offset_to_utf16("hello", 3), Some(3));
        assert_eq!(char_offset_to_utf16("hello", 5), Some(5)); // 文本末尾
        assert_eq!(char_offset_to_utf16("hello", 6), None); // 越界
                                                            // "say 😀 hi"：😀 占 1 个 codepoint、2 个 UTF-16 码元
        assert_eq!(char_offset_to_utf16("say 😀 hi", 4), Some(4)); // 😀 起点
        assert_eq!(char_offset_to_utf16("say 😀 hi", 5), Some(6)); // 😀 之后
        assert_eq!(char_offset_to_utf16("你好世界", 2), Some(2)); // BMP 字符两者一致
    }

    /// 剪贴板权威调和（Windows）：词漂移时替换、撞错控件时降级、无权威时原样
    #[test]
    fn reconcile_with_clipboard_primary() {
        // 词漂移但句子含权威词（Edge PDF 实测形态）：句子保留、词替换
        let reconciled = reconcile_with_primary(
            "without loss of reconstruction quality.".to_string(),
            Some("onstruction qu".to_string()),
            Some(5),
            Some("reconstruction".to_string()),
        );
        assert_eq!(reconciled.text, "without loss of reconstruction quality.");
        assert_eq!(reconciled.word.as_deref(), Some("reconstruction"));
        // 词被权威词替换：原偏移指向漂移前的词，作废
        assert_eq!(reconciled.word_offset, None);

        // 句子不含权威词（撞错控件/漂移越界）：降级为仅录入权威文本
        let reconciled = reconcile_with_primary(
            "LiGameAcademy/.github".to_string(),
            Some("ademy".to_string()),
            Some(0),
            Some("reconstruction".to_string()),
        );
        assert_eq!(reconciled.text, "reconstruction");
        assert_eq!(reconciled.word, None);

        // 词本就一致：不产生任何变化
        let reconciled = reconcile_with_primary(
            "the word is here.".to_string(),
            Some("word".to_string()),
            Some(9),
            Some("word".to_string()),
        );
        assert_eq!(reconciled.text, "the word is here.");
        assert_eq!(reconciled.word.as_deref(), Some("word"));
        // 词与句子都未变：偏移继续有效
        assert_eq!(reconciled.word_offset, Some(9));

        // 无权威文本（剪贴板未变/读失败）：原样返回平台结果
        let reconciled = reconcile_with_primary(
            "a sentence.".to_string(),
            Some("sentence".to_string()),
            Some(2),
            None,
        );
        assert_eq!(reconciled.text, "a sentence.");
        assert_eq!(reconciled.word.as_deref(), Some("sentence"));
        let reconciled = reconcile_with_primary(
            "onstruction qu".to_string(),
            None,
            None,
            Some("  ".to_string()), // 空白视为无权威
        );
        assert_eq!(reconciled.text, "onstruction qu");

        // 未取句（降级/原样录入）：以权威文本为准
        let reconciled = reconcile_with_primary(
            "onstruction qu".to_string(),
            None,
            None,
            Some("reconstruction".to_string()),
        );
        assert_eq!(reconciled.text, "reconstruction");
        assert_eq!(reconciled.word, None);
    }

    /// 测试便捷构造：子对象整体算作选中
    fn object(text: &str) -> Option<EmbeddedObject> {
        return Some(EmbeddedObject {
            text: text.to_string(),
            selected: None,
        });
    }

    /// 取出 Expanded 结果的 (句子, 词偏移)，并校验偏移处的 UTF-16 切片确为所选词
    fn expanded_of(result: &EmbeddedCapture) -> (&str, usize) {
        let SentenceCapture::Expanded {
            sentence,
            word_offset,
            ..
        } = &result.capture
        else {
            panic!("expected an expanded sentence, got {:?}", result.capture);
        };
        let word_offset = word_offset.expect("word offset");
        let sentence_utf16: Vec<u16> = sentence.encode_utf16().collect();
        let word_utf16: Vec<u16> = result.word.encode_utf16().collect();
        assert_eq!(
            sentence_utf16[word_offset..word_offset + word_utf16.len()],
            word_utf16[..]
        );
        return (sentence, word_offset);
    }

    /// 完整性：展开后除占位符外逐字符不变（含 CJK 与 astral 字符）；选区端点随之换算
    #[test]
    fn expansion_keeps_text_intact() {
        let context = "Hello \u{FFFC} there, 世界😀 \u{FFFC}\u{FFFC} end.\n";
        let objects = [object("brave 𝒳 new"), object(""), object("𝒴")];
        let expanded = expand_embedded_objects(context, 8, 13, &objects).unwrap();

        let mut expected = String::new();
        let mut replacements = objects.iter().flatten();
        for ch in context.chars() {
            if ch == EMBEDDED_OBJECT_CHAR {
                expected.push_str(&replacements.next().unwrap().text);
            } else {
                expected.push(ch);
            }
        }
        assert_eq!(expanded.context, expected);
        assert_eq!(expanded.context, "Hello brave 𝒳 new there, 世界😀 𝒴 end.\n");

        // 选区是原文的 "there"（字符 [8, 13)）：前面的展开文本含 astral 字符，须按 UTF-16 计
        let start = "Hello brave 𝒳 new ".encode_utf16().count();
        assert_eq!(expanded.sel_start_utf16, start);
        assert_eq!(expanded.sel_end_utf16, start + 5);
    }

    /// 没有占位符时展开是恒等变换
    #[test]
    fn expansion_without_objects_is_identity() {
        let context = "纯文本 😀 no objects.";
        let expanded = expand_embedded_objects(context, 4, 5, &[]).unwrap();
        assert_eq!(expanded.context, context);
        assert_eq!(expanded.sel_start_utf16, 4);
        assert_eq!(expanded.sel_end_utf16, 6);
    }

    /// 未解析的占位符原样保留，其余文本不受影响
    #[test]
    fn expansion_keeps_unresolved_placeholder() {
        let context = "a \u{FFFC} b \u{FFFC} c";
        let expanded = expand_embedded_objects(context, 0, 1, &[None, object("link")]).unwrap();
        assert_eq!(expanded.context, "a \u{FFFC} b link c");
    }

    /// 占位符与解析结果数量不符、选区越界、子对象选区非法时不展开
    #[test]
    fn expansion_rejects_invalid_input() {
        let context = "a \u{FFFC} b";
        assert!(expand_embedded_objects(context, 0, 1, &[]).is_none());
        assert!(expand_embedded_objects(context, 0, 100, &[object("x")]).is_none());
        for selected in [(2, 1), (0, 100)] {
            let objects = [Some(EmbeddedObject {
                text: "xy".to_string(),
                selected: Some(selected),
            })];
            assert!(expand_embedded_objects(context, 2, 3, &objects).is_none());
        }
    }

    /// 选词在普通文本里：句中链接展开，偏移换算到展开后的句子
    #[test]
    fn embedded_sentence_around_plain_word() {
        let context = "First one. See \u{FFFC} for 😀 details here. Next \u{FFFC} end.";
        let start = context.chars().position(|ch| ch == 'd').unwrap();
        let result = capture_with_embedded_objects(
            context,
            "details",
            start,
            &[object("the 𝒳 docs"), object("other")],
            0,
        )
        .unwrap();
        assert_eq!(result.word, "details");
        let (sentence, word_offset) = expanded_of(&result);
        assert_eq!(sentence, "See the 𝒳 docs for 😀 details here.");
        assert_eq!(word_offset, "See the 𝒳 docs for 😀 ".encode_utf16().count());
        assert!(!result.touched_edge);
    }

    /// 选中整个链接：父对象里读到的词只有一个占位符，要还原成链接文字
    #[test]
    fn embedded_word_is_a_whole_link() {
        let result = capture_with_embedded_objects(
            "Click \u{FFFC} now. Bye.",
            "\u{FFFC}",
            6,
            &[object("here")],
            0,
        )
        .unwrap();
        assert_eq!(result.word, "here");
        assert_eq!(expanded_of(&result), ("Click here now.", 6));
    }

    /// 只选链接的一部分：词按子对象内的选区收窄；句中同词多次出现时偏移仍指向选中的那处
    #[test]
    fn embedded_word_is_part_of_a_link() {
        let objects = [Some(EmbeddedObject {
            text: "hello world".to_string(),
            selected: Some((6, 11)),
        })];
        let result =
            capture_with_embedded_objects("world \u{FFFC} end. Bye.", "\u{FFFC}", 6, &objects, 0)
                .unwrap();
        assert_eq!(result.word, "world");
        assert_eq!(expanded_of(&result), ("world hello world end.", 12));
    }

    /// 选区跨过链接：词的两端是普通文本，中间展开
    #[test]
    fn embedded_word_spans_a_link() {
        let context = "Say a \u{FFFC} b now.";
        let result =
            capture_with_embedded_objects(context, "a \u{FFFC} b", 4, &[object("𝒳")], 0).unwrap();
        assert_eq!(result.word, "a 𝒳 b");
        assert_eq!(expanded_of(&result), ("Say a 𝒳 b now.", 4));
    }

    /// 选区以链接开头和结尾，且两端都只选了一部分
    #[test]
    fn embedded_word_starts_and_ends_inside_links() {
        let objects = [
            Some(EmbeddedObject {
                text: "abc def".to_string(),
                selected: Some((4, 7)),
            }),
            Some(EmbeddedObject {
                text: "ghi jkl".to_string(),
                selected: Some((0, 3)),
            }),
        ];
        let result = capture_with_embedded_objects(
            "\u{FFFC} mid \u{FFFC} tail.",
            "\u{FFFC} mid \u{FFFC}",
            0,
            &objects,
            0,
        )
        .unwrap();
        assert_eq!(result.word, "def mid ghi");
        assert_eq!(expanded_of(&result), ("abc def mid ghi jkl tail.", 4));
    }

    /// 图片等非文本对象展开为空串：其余文本逐字符保留
    #[test]
    fn embedded_image_expands_to_nothing() {
        let result =
            capture_with_embedded_objects("Hello \u{FFFC} there.", "there", 8, &[object("")], 0)
                .unwrap();
        assert_eq!(expanded_of(&result), ("Hello  there.", 7));
    }

    /// 未解析的占位符落进句子或词：降级，不录入缺字的文本
    #[test]
    fn embedded_unresolved_in_sentence_degrades() {
        assert!(capture_with_embedded_objects(
            "Hello \u{FFFC} there. Bye.",
            "there",
            8,
            &[None],
            0
        )
        .is_none());
        assert!(
            capture_with_embedded_objects("Click \u{FFFC} now.", "\u{FFFC}", 6, &[None], 0)
                .is_none()
        );
        assert!(capture_with_embedded_objects(
            "Say a \u{FFFC} b now.",
            "a \u{FFFC} b",
            4,
            &[None],
            0
        )
        .is_none());
    }

    /// 未解析的占位符在别的句子里：不影响本句
    #[test]
    fn embedded_unresolved_in_other_sentence_is_ignored() {
        let result = capture_with_embedded_objects(
            "Hello there. Next \u{FFFC} one.",
            "there",
            6,
            &[None],
            0,
        )
        .unwrap();
        assert_eq!(expanded_of(&result), ("Hello there.", 6));
    }

    /// 词含占位符时偏移必须可信：原文切片与词不一致就降级，不用展开结果猜
    #[test]
    fn embedded_word_with_stale_offset_degrades() {
        assert!(capture_with_embedded_objects(
            "Click \u{FFFC} now.",
            "\u{FFFC}",
            0,
            &[object("here")],
            0
        )
        .is_none());
    }

    /// 普通词的偏移失真时沿用唯一出现兜底，与不含占位符的路径一致
    #[test]
    fn embedded_plain_word_uses_unique_occurrence_fallback() {
        let result = capture_with_embedded_objects(
            "See \u{FFFC} for details.",
            "details",
            0,
            &[object("docs")],
            0,
        )
        .unwrap();
        assert_eq!(expanded_of(&result), ("See docs for details.", 13));
    }

    /// 选区跨句：与普通路径一样原样录入所选文本，占位符已展开
    #[test]
    fn embedded_selection_across_sentences() {
        let result = capture_with_embedded_objects(
            "One. Two \u{FFFC}. Three.",
            "Two \u{FFFC}. Three",
            5,
            &[object("𝒳")],
            0,
        )
        .unwrap();
        assert_eq!(result.word, "Two 𝒳. Three");
        let SentenceCapture::SelectionAsIs(text) = &result.capture else {
            panic!("expected the selection as is, got {:?}", result.capture);
        };
        assert_eq!(text, "Two 𝒳. Three");
    }

    /// 触边判定基于展开后的上下文
    #[test]
    fn embedded_touched_edge() {
        let result =
            capture_with_embedded_objects("cut \u{FFFC} word", "word", 6, &[object("off")], 100)
                .unwrap();
        assert!(result.touched_edge);
        assert_eq!(expanded_of(&result), ("cut off word", 8));
    }

    /// 子串在文本中的 UTF-16 偏移（测试辅助）
    fn utf16_of(text: &str, needle: &str) -> usize {
        let byte = text.find(needle).expect("needle not found");
        return text[..byte].encode_utf16().count();
    }

    /// 块片段组装：片段间以换行连接，选区偏移按“前面的片段 + 换行”累计
    #[test]
    fn assemble_block_segments_maps_the_selection_offset() {
        let segments = ["first line", "second line", "third"];
        let (block, offset) = assemble_block_segments(&segments, 1, 3);
        assert_eq!(block, "first line\nsecond line\nthird");
        assert_eq!(offset, 11 + 3);

        let (head_block, head_offset) = assemble_block_segments(&segments, 0, 0);
        assert_eq!(head_block, block);
        assert_eq!(head_offset, 0);
    }

    /// 块片段直接取句：引用标注把段落切成片段时也要接回整句
    /// （PDF 文本层在链接标注处切分，段落会停在 "…memory ["）
    #[test]
    fn capture_in_segments_rejoins_citations() {
        let segments = [
            "Recurrent neural networks, long short-term memory [",
            "13] and gated recurrent networks.",
        ];
        let offset = utf16_of(segments[0], "memory");
        let (capture, word) = capture_in_segments(&segments, 0, offset, "memory").unwrap();
        assert_eq!(word, "memory");
        let SentenceCapture::Expanded { sentence, .. } = capture else {
            panic!("expected an expanded sentence");
        };
        assert_eq!(
            sentence,
            "Recurrent neural networks, long short-term memory [13] and gated recurrent networks."
        );
    }

    /// 实机样本（arXiv 2508.10104，Chrome PDF 查看器）：AX 把同一个视觉行在每处行内链接
    /// 切成独立片段，并在片段边界插入 CRLF / U+0002（行尾软连字符）。取句必须跨过这些
    /// 结构性切口，否则句子停在第 1 个引用处。
    #[test]
    fn joins_citation_fragments_split_by_the_pdf_provider() {
        let segments = [
            "pervised pre-training approaches for text data. Examples include word embeddings (Mikolov et al., 2013;",
            "\r\nBojanowski et al., ",
            "2017), sentence representations (",
            "Devlin et al., ",
            "2018; ",
            "Liu et al., ",
            "2019), and plain lan\u{2}",
            "guage models (Mikolov et al., 2010; Zaremba et al., 2014). In contrast, computer vision presents greater\r\n",
            "challenges due to the continuous nature of the signal.",
        ];
        const EXPECTED: &str =
            "Examples include word embeddings (Mikolov et al., 2013; Bojanowski et al., 2017), \
             sentence representations (Devlin et al., 2018; Liu et al., 2019), and plain language \
             models (Mikolov et al., 2010; Zaremba et al., 2014).";
        // 句子中任意位置取词都应得到同一句：正向（第一个引用处）与反向（跨过引用往回）
        // 两种拼接方向都要过
        for (segment, word) in [
            (0usize, "embeddings"),
            (1, "Bojanowski"),
            (2, "representations"),
        ] {
            let offset = utf16_of(segments[segment], word);
            let (capture, _) = capture_in_segments(&segments, segment, offset, word).unwrap();
            let SentenceCapture::Expanded { sentence, .. } = capture else {
                panic!("expected an expanded sentence for {word}");
            };
            assert_eq!(sentence, EXPECTED, "anchor word {word}");
        }
    }

    /// 实机样本（arXiv 1810.04805，Chrome PDF 查看器）：提供方把行尾软连字符输出成
    /// 片段内部的 U+0002（imple + U+0002 + mentation）。切句前必须先删掉——UAX #29 会把它
    /// 当成句子边界，句子被从词中间劈开
    #[test]
    fn soft_breaks_inside_a_segment_do_not_split_the_sentence() {
        let segments = [
            "Because the use of Transformers has become common and our im\u{2}plementation is \
             almost identical to the original, we will omit an exhaustive background \
             descrip\u{2}tion of the original.",
        ];
        let offset = utf16_of(segments[0], "Transformers");
        let (capture, _) = capture_in_segments(&segments, 0, offset, "Transformers").unwrap();
        let SentenceCapture::Expanded { sentence, .. } = capture else {
            panic!("expected an expanded sentence");
        };
        assert_eq!(
            sentence,
            "Because the use of Transformers has become common and our implementation is almost \
             identical to the original, we will omit an exhaustive background description of the \
             original."
        );
    }

    /// 上下文是常驻的：句子后面还有一句时，即使当前句已经收住也要给出（界面靠它
    /// 让用户看见边界、并按需并入）。这里验证并入的是“下一句”这个语义单位
    #[test]
    fn context_after_holds_the_next_sentence() {
        let segments = ["The model is fast. ", "It runs on CPUs."];
        let offset = utf16_of(segments[0], "model");
        let (capture, _) = capture_in_segments(&segments, 0, offset, "model").unwrap();
        let SentenceCapture::Expanded {
            sentence, context, ..
        } = capture
        else {
            panic!("expected an expanded sentence");
        };
        assert_eq!(sentence, "The model is fast.");
        assert!(context.before.is_empty());
        assert_eq!(context.after, vec!["It runs on CPUs.".to_string()]);
    }

    /// 上文同样常驻：取句子之前最近的一句，近端排第一
    #[test]
    fn context_before_holds_the_previous_sentence() {
        let segments = ["First one. Second one. Third one."];
        let offset = utf16_of(segments[0], "Second");
        let (capture, _) = capture_in_segments(&segments, 0, offset, "Second").unwrap();
        let SentenceCapture::Expanded {
            sentence, context, ..
        } = capture
        else {
            panic!("expected an expanded sentence");
        };
        assert_eq!(sentence, "Second one.");
        assert_eq!(context.before, vec!["First one.".to_string()]);
        assert_eq!(context.after, vec!["Third one.".to_string()]);
    }

    /// 段落是否被切断：内嵌对象前的空白、引用标注处的开括号都算；标题与单元格不算
    #[test]
    fn block_looks_cut_detection() {
        assert!(block_looks_cut("…a sequence of hidden states "));
        assert!(block_looks_cut("…long short-term memory ["));
        assert!(block_looks_cut("…the Transformer ("));
        assert!(!block_looks_cut("1 Introduction"));
        assert!(!block_looks_cut("91"));
        assert!(!block_looks_cut("A complete sentence."));
    }

    /// 折行连字符：词里带 U+2010、文本里没有时也要能对上
    #[test]
    fn hyphenated_word_matches_unhyphenated_text() {
        let segments = ["Examples include word embeddings (Mikolov et al., 2013)."];
        let (capture, word) = capture_in_segments(&segments, 0, 22, "em\u{2010}bedding").unwrap();
        assert_eq!(word, "embeddings");
        let SentenceCapture::Expanded { sentence, .. } = capture else {
            panic!("expected an expanded sentence");
        };
        assert!(sentence.starts_with("Examples include word embeddings"));
    }

    /// 少报末尾字符的选区要补到完整单词：Chromium 把 "Examples" 报成 "Example"
    #[test]
    fn partial_word_is_completed() {
        let segments = ["Examples include word embeddings (Mikolov et al., 2013)."];
        let (capture, word) = capture_in_segments(&segments, 0, 0, "Example").unwrap();
        assert_eq!(word, "Examples");
        let SentenceCapture::Expanded { sentence, .. } = capture else {
            panic!("expected an expanded sentence");
        };
        assert!(sentence.starts_with("Examples include"));
    }

    /// 缩写处的伪句末：UAX #29 把 "Fig. 2" 切成两段，要并回同一句
    #[test]
    fn abbreviation_break_is_rejoined() {
        let context = "Overall they win on dense tasks, as visible in Fig. 2. Next one.";
        let offset = utf16_of(context, "visible");
        let (capture, _) =
            capture_in_context(context, "visible", &[offset as isize], 0, false).unwrap();
        let SentenceCapture::Expanded { sentence, .. } = capture else {
            panic!("expected an expanded sentence");
        };
        assert_eq!(
            sentence,
            "Overall they win on dense tasks, as visible in Fig. 2."
        );
    }

    /// 标题不是伪句末：下一段以大写开头，不并回
    #[test]
    fn heading_is_not_merged_into_the_next_sentence() {
        let context = "Introduction\nRecurrent models typically factor computation.";
        let offset = utf16_of(context, "Introduction");
        let (capture, _) =
            capture_in_context(context, "Introduction", &[offset as isize], 0, false).unwrap();
        let SentenceCapture::Expanded { sentence, .. } = capture else {
            panic!("expected an expanded sentence");
        };
        assert_eq!(sentence, "Introduction");
    }

    /// 块拼接：行内公式被拆成多个字符块（"h" "t" "−" "1"），仍要接回同一句。
    /// 回归：公式之后的符号/数字曾被当成新块，句子在公式处被截断
    #[test]
    fn block_join_reconnects_formula_fragments() {
        let raw = "they generate a sequence of hidden states \nh\nt\n, as a function of the previous hidden state \nh\nt\n−\n1\n and the input for position \nt\n. This inherently";
        let offset = utf16_of(raw, "function");
        let (block, block_sel) = join_block_around_anchor(raw, offset, offset + 8).unwrap();
        assert_eq!(
            block,
            "they generate a sequence of hidden states h t, as a function of the previous hidden \
             state h t − 1 and the input for position t. This inherently"
        );
        assert_eq!(block_sel, utf16_of(&block, "function"));
        // 拼回块后句子完整：不被公式切断，也不把下一句并进来
        let (capture, _) =
            capture_in_context(&block, "function", &[block_sel as isize], 0, false).unwrap();
        let SentenceCapture::Expanded { sentence, .. } = capture else {
            panic!("expected an expanded sentence");
        };
        assert_eq!(
            sentence,
            "they generate a sequence of hidden states h t, as a function of the previous hidden \
             state h t − 1 and the input for position t."
        );
    }

    /// 块拼接：网页段落本身是完整块，下一行的标题（大写开头）不被并进句子。
    /// 回归：早期实现把换行抹平后重新分句，“…lazy dog Results We evaluate…” 会被并成一句
    #[test]
    fn block_join_stops_at_heading() {
        let raw = "The quick brown fox jumps over the lazy dog\nResults\nWe evaluate the model on three datasets.";
        let offset = utf16_of(raw, "lazy");
        let (block, block_sel) = join_block_around_anchor(raw, offset, offset + 4).unwrap();
        assert_eq!(block, "The quick brown fox jumps over the lazy dog");
        assert_eq!(block_sel, offset);
    }

    /// 块拼接：PDF 的软换行（行切开同一个句子）要接回去，行间只留一个空格
    #[test]
    fn block_join_continues_soft_wrapped_lines() {
        let raw = "3.2 Input Layer. The first thing to build a model is to\r\nprepare inputs with abundant information, such as User features\r\ninclude user ID and other user information. Next one.";
        let offset = utf16_of(raw, "abundant");
        let (block, block_sel) = join_block_around_anchor(raw, offset, offset + 8).unwrap();
        assert_eq!(
            block,
            "3.2 Input Layer. The first thing to build a model is to prepare inputs with abundant \
             information, such as User features include user ID and other user information. Next one."
        );
        assert_eq!(block_sel, utf16_of(&block, "abundant"));
        // 折行后的块里能取回跨行整句（PDF 选词取句的核心能力）
        let (capture, _) =
            capture_in_context(&block, "abundant", &[block_sel as isize], 0, false).unwrap();
        let SentenceCapture::Expanded { sentence, .. } = capture else {
            panic!("expected an expanded sentence");
        };
        assert_eq!(
            sentence,
            "The first thing to build a model is to prepare inputs with abundant information, \
             such as User features include user ID and other user information."
        );
    }

    /// 块拼接：上一行已经结束句子时不再向上合并（哪怕下一行以小写开头）
    #[test]
    fn block_join_stops_after_sentence_end() {
        let raw = "Done.\nnext line here";
        let offset = utf16_of(raw, "line");
        let (block, block_sel) = join_block_around_anchor(raw, offset, offset + 4).unwrap();
        assert_eq!(block, "next line here");
        assert_eq!(block_sel, 5);
    }

    /// 块拼接：行尾软连字符合并时去掉连字符，选区偏移同步前移
    #[test]
    fn block_join_dehyphenates() {
        let raw = "The per-\ncentage improves.";
        let offset = utf16_of(raw, "centage");
        let (block, block_sel) = join_block_around_anchor(raw, offset, offset + 7).unwrap();
        assert_eq!(block, "The percentage improves.");
        assert_eq!(block_sel, utf16_of(&block, "centage"));
    }

    /// 块拼接：Chromium 在折行处插入 U+2010 连字符（原文没有），也要去掉
    #[test]
    fn block_join_dehyphenates_unicode_hyphen() {
        let raw = "such as lan‐
guage modeling";
        let offset = utf16_of(raw, "lan");
        let (block, block_sel) = join_block_around_anchor(raw, offset, offset + 3).unwrap();
        assert_eq!(block, "such as language modeling");
        assert_eq!(block_sel, 8);
    }

    /// 块拼接：缩写（Fig. / Eq. / et al.）不是句末，跨行仍要接回去
    #[test]
    fn block_join_keeps_abbreviation_lines_together() {
        let raw = "as shown in Fig.\n3 the curve rises";
        let offset = utf16_of(raw, "curve");
        let (block, _) = join_block_around_anchor(raw, offset, offset + 5).unwrap();
        assert_eq!(block, "as shown in Fig. 3 the curve rises");
    }

    /// 块拼接：空行不算块边界（Chromium 的 PDF 查看器用连续换行分隔视觉行）
    #[test]
    fn block_join_ignores_blank_lines() {
        let raw = "The quick brown fox\n\njumps over the lazy dog";
        let offset = utf16_of(raw, "lazy");
        let (block, _) = join_block_around_anchor(raw, offset, offset + 4).unwrap();
        assert_eq!(block, "The quick brown fox jumps over the lazy dog");
    }

    /// 块拼接：CJK 折行直接相接，不补空格
    #[test]
    fn block_join_cjk_without_space() {
        let raw = "这是第一句话\n第二行继续写完";
        let offset = utf16_of(raw, "第二行");
        let (block, block_sel) = join_block_around_anchor(raw, offset, offset + 3).unwrap();
        assert_eq!(block, "这是第一句话第二行继续写完");
        assert_eq!(block_sel, 6);
    }

    /// 块拼接：手动跨行选择时整段保留，不受“不能并块”的规则约束
    #[test]
    fn block_join_keeps_multiline_selection() {
        let raw = "Results\nThe quick brown fox jumps over the lazy dog";
        let start = utf16_of(raw, "Results");
        let end = utf16_of(raw, "lazy") + 4;
        let (block, block_sel) = join_block_around_anchor(raw, start, end).unwrap();
        assert_eq!(block, "Results The quick brown fox jumps over the lazy dog");
        assert_eq!(block_sel, 0);
    }

    /// PRIMARY 变化判定：首次与内容变化都算变化，内容不变不算；变回旧内容也算变化
    #[test]
    fn primary_change_detection() {
        let mut last = None;
        assert!(remember_primary(&mut last, "old"));
        assert!(!remember_primary(&mut last, "old"));
        assert!(remember_primary(&mut last, "new"));
        assert!(!remember_primary(&mut last, "new"));
        assert!(remember_primary(&mut last, "old"));
        assert_eq!(last.as_deref(), Some("old"));
    }
}
