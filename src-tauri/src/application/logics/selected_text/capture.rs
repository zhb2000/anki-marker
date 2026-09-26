//! 跨平台共享的取句纯逻辑：锚点探测（校验门控）→ 跨句保留 → 单句扩展。
//!
//! 三个平台的取句链路形态一致：平台层（macOS AX / Windows UIA / Linux AT-SPI）
//! 负责取得“选中的词”与“选区周边的上下文窗口”，并以 UTF-16 码元偏移给出
//! 若干候选锚点；本模块在上下文中校验锚点并切出句子。偏移坐标系因提供方而
//! 异（详见 find_anchor），因此所有候选锚点都要经“该位置的 UTF-16 子串等于
//! 所选文本”校验后才采用——宁可降级为仅录词，也不切出错误的句子。

use crate::application::logics::sentence::{
    count_intersecting_sentences, find_sentence_with_range,
};

/// 取句结果：单句时为用户所选词扩展出的整句；用户手动跨句选择时为其所选原文。
#[derive(Debug)]
pub enum SentenceCapture {
    /// 选区落在单个句子内，已扩展为整句（前端可预选命中的单词）
    Expanded(String),
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
) -> Option<(SentenceCapture, bool)> {
    let sel_start_utf16 = find_anchor(context, word, anchor_candidates)?;
    let sel_end_utf16 = sel_start_utf16 + word.encode_utf16().count();

    // 只扩展、不丢弃：选区横跨多个句子时原样录入用户所选文本，不做分句截断。
    // 典型场景：Chrome PDF 中手动选中跨行的完整句子——PDF 视觉行间的 \n 使
    // UAX #29（SB4：CR/LF 后强制分句）把整句切为多行碎片，若仍取"与选区起点
    // 相交的句子"就只会录入第一行，丢弃了用户明确选择的内容
    if count_intersecting_sentences(context, sel_start_utf16, sel_end_utf16) > 1 {
        return Some((SentenceCapture::SelectionAsIs(word.trim().to_string()), false));
    }
    let (sentence, sent_start, sent_end) =
        find_sentence_with_range(context, sel_start_utf16, sel_end_utf16)?;
    let touched_edge = (sent_start == 0 && window_loc > 0) || sent_end == context.len();
    return Some((SentenceCapture::Expanded(sentence), touched_edge));
}

/// 在上下文中定位所选文本的 UTF-16 起点（锚点探测，校验门控）。
///
/// 提供方的偏移坐标系存在已知怪癖：选区偏移与窗口文本的坐标系可能不一致
/// （macOS 上如 Obsidian 阅读模式：偏移是文档级的、返回文本却是块级的）。
/// 因此不盲信偏移，按优先级尝试候选锚点（由平台层给出，如窗口偏移 → 文档级
/// 偏移 → 0），每个候选以“该位置的 UTF-16 子串等于所选文本”校验后才采用；
/// 全部失败时退到“所选文本的唯一出现位置”（零次出现无法定位、多次出现有
/// 歧义，均不采用）。仍失败返回 None——宁可降级为仅录词，也不切出错误的句子。
pub fn find_anchor(context: &str, word: &str, candidates: &[isize]) -> Option<usize> {
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

#[cfg(test)]
mod tests {
    use super::{capture_in_context, char_offset_to_utf16, find_anchor, SentenceCapture};

    /// 锚点候选 1：提供方忠实响应请求窗口（窗口偏移有效）
    #[test]
    fn anchor_window_offset() {
        // 文档 "Hello world. This is a test."，窗口 loc=9，word "This" 在文档偏移 13
        let context = "ld. This is a te";
        // 候选 1：13 - 9 = 4，context[4..8] == "This"
        assert_eq!(find_anchor(context, "This", &[13 - 9, 13, 0]), Some(4));
    }

    /// 锚点候选 2：提供方忽略窗口起点、从文档开头返回文本（文档级偏移）
    #[test]
    fn anchor_document_offset() {
        let context = "Hello world. This is";
        // 候选 1（13-9=4）命中 "o wo" 不匹配；候选 2（13）命中 "This"
        assert_eq!(find_anchor(context, "This", &[13 - 9, 13, 0]), Some(13));
    }

    /// 锚点候选 3：提供方从选区开始返回文本
    #[test]
    fn anchor_selection_start() {
        let context = "This is a test.";
        // 候选 1（4）与候选 2（13，越界）均失败；候选 3（0）命中
        assert_eq!(find_anchor(context, "This", &[13 - 9, 13, 0]), Some(0));
    }

    /// 锚点兜底：偏移完全失真时靠唯一出现位置定位；多处出现有歧义返回 None
    #[test]
    fn anchor_unique_occurrence() {
        let context = "jumped over. The fox runs. End.";
        assert_eq!(
            find_anchor(context, "fox", &[9999 - 9000, 9999, 0]),
            Some("jumped over. The ".encode_utf16().count())
        );
        // 两次出现 → 歧义，不采用（上下文开头放非匹配内容，避免候选 0 先命中）
        assert_eq!(find_anchor("a fox and fox", "fox", &[999, 9999, 0]), None);
    }

    /// 所选文本不在上下文中：返回 None（降级为仅录词，不切错句）
    #[test]
    fn anchor_not_found() {
        assert_eq!(find_anchor("Hello world.", "zebra", &[0]), None);
    }

    /// 含 emoji（UTF-16 代理对）的所选文本：按 UTF-16 码元校验与定位
    #[test]
    fn anchor_with_emoji() {
        // "say " 共 4 个 UTF-16 码元，😀 占 2 个
        assert_eq!(find_anchor("say 😀 hi.", "😀", &[4]), Some(4));
    }

    /// 块级上下文取句（Obsidian 阅读模式场景）：偏移是文档级的、
    /// 上下文窗口只返回段落块，靠锚点探测取到块内完整句子
    #[test]
    fn capture_in_block_context() {
        let context = "First sentence here. The target word is fox.";
        let (capture, touched_edge) =
            capture_in_context(context, "fox", &[5000 - 4000, 5000, 0], 4000)
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
        let (capture, touched_edge) =
            capture_in_context(context, word, &[5], 0).expect("should capture");
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
}
