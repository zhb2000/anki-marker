//! 跨平台共享的取句纯逻辑：锚点探测（校验门控）→ 跨句保留 → 单句扩展。
//!
//! 三个平台的取句链路形态一致：平台层（macOS AX / Windows UIA / Linux AT-SPI）
//! 负责取得“选中的词”与“选区周边的上下文窗口”，并以 UTF-16 码元偏移给出
//! 若干候选锚点；本模块在上下文中校验锚点并切出句子。偏移坐标系因提供方而
//! 异（详见 find_anchor），因此所有候选锚点都要经“该位置的 UTF-16 子串等于
//! 所选文本”校验后才采用——宁可降级为仅录词，也不切出错误的句子。

#[cfg(any(target_os = "macos", target_os = "windows", test))]
use super::SelectedContext;
use crate::application::logics::sentence::{
    count_intersecting_sentences, find_sentence_with_range,
};

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
    let (sentence, sent_start, sent_end) =
        find_sentence_with_range(context, sel_start_utf16, sel_end_utf16)?;
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
                text: primary,
                word: None,
                word_offset: None,
            }
        }
        None => SelectedContext {
            text: primary,
            word: None,
            word_offset: None,
        },
    };
}

#[cfg(test)]
mod tests {
    use super::{
        capture_in_context, capture_with_embedded_objects, char_offset_to_utf16,
        expand_embedded_objects, find_anchor, reconcile_with_primary, remember_primary,
        EmbeddedCapture, EmbeddedObject, SentenceCapture, EMBEDDED_OBJECT_CHAR,
    };

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
