//! 取句结果的文本清理：去除正文外字符（伪影）、折叠空白、抹换行与截断判定。

/// 去掉提供方文本里不属于正文的字符：控制字符（Chromium 的 PDF 查看器把行尾连字符
/// 输出为 U+0002，如 `per\u{2}centage`）与软连字符 U+00AD。放在取句之后、只处理输出，
/// 不影响取句用的偏移。
pub(super) fn strip_text_artifacts(text: &str) -> String {
    return text
        .chars()
        .filter(|c| *c != '\u{AD}' && !(c.is_control() && !c.is_whitespace()))
        .collect();
}

/// `strip_text_artifacts` 的偏移保真版本：去伪影的同时把 UTF-16 偏移换算到新文本，
/// **文本内容本身只会被删去伪影**。偏移落在被删字符处时映射到其前一个保留字符之后；
/// 偏移落在代理对中间或越界时返回 None，交由前端回退到按字符串匹配。
pub(super) fn strip_text_artifacts_with_offset(
    text: &str,
    offset: Option<usize>,
) -> (String, Option<usize>) {
    let kept = |c: char| c != '\u{AD}' && !(c.is_control() && !c.is_whitespace());
    let Some(offset) = offset else {
        return (strip_text_artifacts(text), None);
    };
    let mut output = String::with_capacity(text.len());
    let mut source_utf16 = 0usize;
    let mut output_utf16 = 0usize;
    let mut mapped: Option<usize> = None;
    for c in text.chars() {
        if mapped.is_none() && source_utf16 == offset {
            mapped = Some(output_utf16);
        }
        source_utf16 += c.len_utf16();
        // 已越过偏移却仍未记下映射：偏移落在这个字符内部（代理对中间），不可映射
        if mapped.is_none() && source_utf16 > offset {
            return (strip_text_artifacts(text), None);
        }
        if kept(c) {
            output.push(c);
            output_utf16 += c.len_utf16();
        }
    }
    if mapped.is_none() && source_utf16 == offset {
        mapped = Some(output_utf16);
    }
    return (output, mapped);
}

/// 句子是否看起来被上下文截断：以小写字母开头（句子不会以小写开头，说明左侧被截），
/// 或不以句末标点（含其后的引号、括号）结尾（右侧被截）。仅用于决定是否扩大上下文重试。
pub(super) fn looks_truncated(sentence: &str) -> bool {
    let starts_lowercase = sentence.chars().next().is_some_and(char::is_lowercase);
    let trimmed = sentence.trim_end_matches(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '”' | '’' | ')' | ']' | '»' | '」' | '』' | '）'
            )
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
pub(super) fn flatten_line_breaks(text: &str) -> String {
    return text
        .chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect();
}

/// 折叠连续空白为单个空格（抹换行产生的双空格、行首行尾空白一并收拾）。
pub(super) fn collapse_whitespace(text: &str) -> String {
    return text.split_whitespace().collect::<Vec<_>>().join(" ");
}

#[cfg(test)]
mod tests {
    use super::strip_text_artifacts_with_offset;

    /// 去伪影的偏移保真版本：**文本完整保留**（只删伪影），偏移随之前移；
    /// 偏移缺失时原样去伪影，落在代理对中间时偏移作废。
    /// 回归：早期实现在到达偏移处 break，把句子的后半段截掉了（选中 design 只录入 We）
    #[test]
    fn strip_artifacts_keeps_the_whole_text() {
        let sentence = "We design a Transformer-based encoder-decoder model.";
        assert_eq!(
            strip_text_artifacts_with_offset(sentence, Some(3)),
            (sentence.to_string(), Some(3))
        );
        assert_eq!(
            strip_text_artifacts_with_offset(sentence, None),
            (sentence.to_string(), None)
        );
        // 偏移在词尾（句子末尾）也完整保留文本
        let tail = sentence.encode_utf16().count();
        assert_eq!(
            strip_text_artifacts_with_offset(sentence, Some(tail)),
            (sentence.to_string(), Some(tail))
        );
    }

    /// 去伪影时偏移跟随：词前的软连字符被删掉后，偏移要前移一位
    #[test]
    fn strip_artifacts_maps_offset_past_removed_chars() {
        let text = "per\u{AD}centage improves.";
        // "per" + 软连字符 + "centage " = 12 个 UTF-16 码元；去掉软连字符后落在 11
        let offset = "per\u{AD}centage ".encode_utf16().count();
        assert_eq!(offset, 12);
        assert_eq!(
            strip_text_artifacts_with_offset(text, Some(offset)),
            ("percentage improves.".to_string(), Some(11))
        );
    }

    /// 偏移落在代理对中间：映射作废，但文本仍完整返回
    #[test]
    fn strip_artifacts_rejects_offset_inside_surrogate_pair() {
        let text = "say 😀 hi";
        assert_eq!(
            strip_text_artifacts_with_offset(text, Some(5)),
            (text.to_string(), None)
        );
        assert_eq!(
            strip_text_artifacts_with_offset(text, Some(4)),
            (text.to_string(), Some(4))
        );
    }

    /// 去除正文外字符：U+0002（Chromium PDF 的行尾连字符）、软连字符；普通空白与标点保留
    #[test]
    fn strip_text_artifacts_removes_hyphenation_marks() {
        assert_eq!(
            super::strip_text_artifacts("per\u{2}centage param\u{AD}eters"),
            "percentage parameters"
        );
        assert_eq!(
            super::strip_text_artifacts("a\tb\nc, d-e."),
            "a\tb\nc, d-e."
        );
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
        assert!(looks_truncated(
            "prepare inputs with abundant information, such as "
        )); // 左右都被截
        assert!(looks_truncated("We take a closer look at each")); // 右侧被截
        assert!(looks_truncated(
            "scale to improve performance in recommendation presents."
        )); // 左侧被截
        assert!(looks_truncated("Introduction")); // 标题：无句末标点
    }
}
