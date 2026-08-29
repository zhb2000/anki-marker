//! 句子切分：在上下文文本中定位选区所在的句子（选词取句的核心逻辑）。
//!
//! 选区偏移来自 macOS 辅助功能 API 的 CFRange，按 UTF-16 码元计（Cocoa 惯例），
//! 而 Rust 字符串按 UTF-8 存储，切分前需先把选区偏移换算为 UTF-8 字节偏移。
//! 分句规则使用 unicode-segmentation 实现的 Unicode 文本分段算法（UAX #29），
//! 英文句读（. ! ?）与 CJK 句读（。！？）均可正确切分。

use unicode_segmentation::UnicodeSegmentation;

/// 在上下文文本 `context` 中找到与选区 `[sel_start_utf16, sel_end_utf16)`
/// （UTF-16 码元偏移，来自 macOS AX 的 CFRange）相交的句子，返回 trim 后的句子，
/// 以及命中句子（trim 前）在 `context` 中的字节区间 `[start, end)`，供调用方
/// 判断句子是否触及上下文窗口边缘（触边意味着窗口可能截断了句子，需扩大窗口重试）。
/// 选区非法（start>=end 或越界）或无相交句子时返回 None。
pub fn find_sentence_with_range(
    context: &str,
    sel_start_utf16: usize,
    sel_end_utf16: usize,
) -> Option<(String, usize, usize)> {
    if sel_start_utf16 >= sel_end_utf16 {
        return None;
    }
    let sel_start = utf16_offset_to_byte(context, sel_start_utf16)?;
    let sel_end = utf16_offset_to_byte(context, sel_end_utf16)?;
    // unicode-segmentation 仅提供不带索引的 unicode_sentences()，自行累计字节偏移
    let mut sent_start = 0usize;
    for sentence in context.unicode_sentences() {
        let sent_end = sent_start + sentence.len();
        // 字节区间 [sent_start, sent_end) 与选区 [sel_start, sel_end) 相交即命中；
        // 分句结果按序遍历，首个命中即与选区起点相交的句子
        if sent_start < sel_end && sel_start < sent_end {
            return Some((sentence.trim().to_string(), sent_start, sent_end));
        }
        sent_start = sent_end;
    }
    return None;
}

/// 统计与选区 `[sel_start_utf16, sel_end_utf16)`（UTF-16 码元偏移）相交的句子数量。
/// 选区非法时返回 0。用于判断用户是否手动跨句选择（>1 时应原样保留所选文本）。
pub fn count_intersecting_sentences(
    context: &str,
    sel_start_utf16: usize,
    sel_end_utf16: usize,
) -> usize {
    if sel_start_utf16 >= sel_end_utf16 {
        return 0;
    }
    let sel_start = match utf16_offset_to_byte(context, sel_start_utf16) {
        Some(value) => value,
        None => return 0,
    };
    let sel_end = match utf16_offset_to_byte(context, sel_end_utf16) {
        Some(value) => value,
        None => return 0,
    };
    let mut count = 0usize;
    let mut sent_start = 0usize;
    for sentence in context.unicode_sentences() {
        let sent_end = sent_start + sentence.len();
        if sent_start < sel_end && sel_start < sent_end {
            count += 1;
        }
        sent_start = sent_end;
        if sent_start >= sel_end {
            break; // 之后的句子不可能再与选区相交
        }
    }
    return count;
}

/// 将 UTF-16 码元偏移换算为 UTF-8 字节偏移。
///
/// 偏移越过文本末尾、或落在代理对（surrogate pair）中间时返回 None。
fn utf16_offset_to_byte(s: &str, utf16_offset: usize) -> Option<usize> {
    let mut utf16_pos = 0usize;
    for (byte_index, ch) in s.char_indices() {
        if utf16_pos == utf16_offset {
            return Some(byte_index);
        }
        utf16_pos += ch.len_utf16();
        if utf16_pos > utf16_offset {
            return None; // 偏移落在代理对中间，视为非法
        }
    }
    if utf16_pos == utf16_offset {
        return Some(s.len());
    }
    return None;
}

#[cfg(test)]
mod tests {
    use super::{count_intersecting_sentences, find_sentence_with_range};

    /// find_sentence_with_range 的测试便捷包装：只取 trim 后的句子文本
    fn find_sentence(context: &str, sel_start_utf16: usize, sel_end_utf16: usize) -> Option<String> {
        return find_sentence_with_range(context, sel_start_utf16, sel_end_utf16)
            .map(|(sentence, _, _)| sentence);
    }

    /// find_sentence_with_range 额外返回命中句子（trim 前）的字节区间
    #[test]
    fn sentence_with_range() {
        let context = "Hello. World.";
        // 首句含尾随空格：trim 后为 "Hello."，区间为 0..7
        assert_eq!(
            find_sentence_with_range(context, 0, 5),
            Some(("Hello.".to_string(), 0, 7))
        );
        // 第二句 "World."：字节区间 7..13
        assert_eq!(
            find_sentence_with_range(context, 7, 12),
            Some(("World.".to_string(), 7, 13))
        );
    }

    /// 英文多句：选区落在中间句时取中间句
    #[test]
    fn english_middle_sentence() {
        let context = "Hello world. This is a test. Goodbye.";
        // "This" 起始于偏移 13（"Hello world. " 共 13 个字符）
        assert_eq!(
            find_sentence(context, 13, 17).as_deref(),
            Some("This is a test.")
        );
    }

    /// CJK 句读（。！？）分句
    #[test]
    fn cjk_sentence() {
        let context = "第一句。第二句！第三句？";
        // 每个 CJK 字符占 1 个 UTF-16 码元，"第二句" 起始于偏移 4
        assert_eq!(find_sentence(context, 4, 7).as_deref(), Some("第二句！"));
    }

    /// 选区在句首与句尾
    #[test]
    fn selection_at_sentence_edges() {
        let context = "One. Two three. Four.";
        // 句首：第二句开头的 "Two"（"One. " 共 5 个字符）
        assert_eq!(find_sentence(context, 5, 8).as_deref(), Some("Two three."));
        // 句尾：第二句结尾的 "three"（含句读）
        assert_eq!(find_sentence(context, 9, 15).as_deref(), Some("Two three."));
    }

    /// 选区横跨两句时取与选区起点相交的句子
    #[test]
    fn selection_spanning_sentences() {
        let context = "Hello. World.";
        // 选区 "lo. Wo" 横跨两句，起点在 "Hello. " 内
        assert_eq!(find_sentence(context, 3, 9).as_deref(), Some("Hello."));
    }

    /// 上下文窗口截断了句首（无句读终结符）时仍返回截断后的可见句
    #[test]
    fn truncated_window() {
        let context = "truncated visible part";
        assert_eq!(
            find_sentence(context, 0, 9).as_deref(),
            Some("truncated visible part")
        );
    }

    /// 含 emoji（UTF-16 代理对）的文本：偏移按 UTF-16 码元计
    #[test]
    fn utf16_surrogate_pairs() {
        // 每个 😀（U+1F600）占 2 个 UTF-16 码元
        let context = "😀😀 hi. Bye.";
        // "hi" 的 UTF-16 偏移为 5..7（两个 emoji 共 4 码元 + 1 空格）
        assert_eq!(find_sentence(context, 5, 7).as_deref(), Some("😀😀 hi."));
        // 选中两个 emoji：0..4
        assert_eq!(find_sentence(context, 0, 4).as_deref(), Some("😀😀 hi."));
        // 选中第二句的 "Bye"：UTF-16 偏移 9..12
        assert_eq!(find_sentence(context, 9, 12).as_deref(), Some("Bye."));
    }

    /// 非法选区返回 None：空选区、start>end、越界、落在代理对中间
    #[test]
    fn invalid_selection() {
        let context = "Hello. World.";
        assert_eq!(find_sentence(context, 5, 5), None); // 空选区
        assert_eq!(find_sentence(context, 8, 3), None); // start > end
        assert_eq!(find_sentence(context, 0, 100), None); // 终点越界
        let emoji = "😀 hi.";
        assert_eq!(find_sentence(emoji, 0, 1), None); // 终点落在代理对中间
        assert_eq!(find_sentence(emoji, 1, 2), None); // 起点落在代理对中间
    }

    /// 相交句子计数：选词取句判断"用户是否手动跨句选择"的依据
    #[test]
    fn intersecting_sentence_count() {
        let context = "Hello world. This is a test. Goodbye.";
        // 单词 "This"：落在单句内
        assert_eq!(count_intersecting_sentences(context, 13, 17), 1);
        // 带句读与尾随空格的选区 "test. "：UAX #29 中尾随空白属于前一句，仍为 1
        assert_eq!(count_intersecting_sentences(context, 22, 28), 1);
        // 手动选中 "test. Goodbye" 跨两句
        assert_eq!(count_intersecting_sentences(context, 22, 34), 2);
        // 非法选区
        assert_eq!(count_intersecting_sentences(context, 5, 5), 0);
        assert_eq!(count_intersecting_sentences(context, 0, 100), 0);
    }

    /// 相交句子计数：PDF 场景——视觉行硬换行使 UAX #29 把跨行整句切为多句，
    /// 手动选中跨行整句时计数 >1，触发"原样保留所选文本"
    #[test]
    fn intersecting_sentence_count_pdf_lines() {
        // 模拟 Chrome PDF 的 AX 文本：句子跨 3 个视觉行，行间为 \n
        let context = "The quick brown fox\njumps over the lazy\ndog. Next one.";
        // 选词 "jumps"（第二行内）：1（取句路径，返回该行——方案2未实施，见调研文档）
        assert_eq!(count_intersecting_sentences(context, 21, 26), 1);
        // 手动选中跨行整句 "The quick brown fox\njumps over the lazy\ndog."：3
        assert_eq!(count_intersecting_sentences(context, 0, 43), 3);
    }
}
