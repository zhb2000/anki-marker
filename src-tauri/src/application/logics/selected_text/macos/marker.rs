//! text marker 提供方（AXTextMarker，如 Safari 网页、Chromium 系）的取词与取句链路。

use super::super::capture::{capture_in_context, SentenceCapture};
use super::super::SelectedContext;
use super::ax::{copy_element_attribute_by_ax, copy_parameterized_attribute_by_ax};
use super::ax_ffi;
use super::text::{
    collapse_whitespace, flatten_line_breaks, looks_truncated, strip_text_artifacts,
    strip_text_artifacts_with_offset,
};

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
fn find_sentence_for_marker_range(
    element: &core_foundation::base::CFType,
    word: &str,
    selection: &core_foundation::base::CFType,
) -> Result<(SentenceCapture, String), String> {
    use core_foundation::base::{CFType, TCFType};

    /// 行导航扩展的行数梯度：每级向前后各扩这么多行；上一级仍被截断才升级。
    /// 一行约 50 到 70 个码元，10 行覆盖绝大多数句子，40 行兜长句。
    const LINE_EXPANSION_STEPS: [usize; 2] = [10, 40];

    let marker_of = |range: &CFType, end: bool| -> Result<CFType, String> {
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
    };
    let selection_start = marker_of(selection, false)?;
    let selection_end = marker_of(selection, true)?;

    // 第一轮：选区起点所在段落（网页的段落本来就是整块文本，绝大多数情况止于此）
    let paragraph = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::PARAGRAPH_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
        selection_start.as_CFTypeRef(),
    )?;
    let paragraph_start = marker_of(&paragraph, false)?;
    let first = capture_in_marker_context(
        element,
        word,
        &paragraph_start,
        &paragraph,
        &selection_start,
        false,
    );
    // 扩展的触发条件：句子看起来被截断，且成因可能是上下文太短——句子贴着上下文边缘，
    // 或上下文里有硬换行（视觉行换行会让 UAX #29 在段落内部强制分句，句子不必触边）。
    // 网页里整块的段落既不含换行、句子也不贴边，不会进入第二轮
    match &first {
        Ok((SentenceCapture::Expanded { sentence, .. }, has_line_break, touched_edge))
            if looks_truncated(sentence) && (*touched_edge || *has_line_break) => {}
        Ok((capture, _, _)) => return Ok((clone_capture(capture), word.to_string())),
        Err(_) => {}
    }

    // 第二轮起：行导航扩展。锚点是选区所在的整行，向前后各走若干行
    let line_start_of = |marker: &CFType, end: bool| -> Result<CFType, String> {
        let line = copy_parameterized_attribute_by_ax(
            element,
            ax_ffi::LINE_TEXT_MARKER_RANGE_PARAMETERIZED_ATTRIBUTE,
            marker.as_CFTypeRef(),
        )?;
        return marker_of(&line, end);
    };
    let (anchor_left, anchor_right) = match (
        line_start_of(&selection_start, false),
        line_start_of(&selection_end, true),
    ) {
        (Ok(left), Ok(right)) => (left, right),
        (Err(error), _) | (_, Err(error)) => {
            return first
                .map(|(capture, _, _)| (capture, word.to_string()))
                .map_err(|first_error| format!("{first_error}; line navigation: {error}"));
        }
    };
    let flat_word = flatten_line_breaks(word);
    let mut best: Option<(SentenceCapture, String)> = None;
    for lines in LINE_EXPANSION_STEPS {
        let left = step_lines(
            element,
            ax_ffi::PREVIOUS_LINE_START_MARKER_PARAMETERIZED_ATTRIBUTE,
            &anchor_left,
            lines,
        );
        let right = step_lines(
            element,
            ax_ffi::NEXT_LINE_END_MARKER_PARAMETERIZED_ATTRIBUTE,
            &anchor_right,
            lines,
        );
        let markers = core_foundation::array::CFArray::from_CFTypes(&[left.clone(), right]);
        let Ok(range) = copy_parameterized_attribute_by_ax(
            element,
            ax_ffi::RANGE_FOR_UNORDERED_MARKERS_PARAMETERIZED_ATTRIBUTE,
            markers.as_CFTypeRef(),
        ) else {
            break;
        };
        match capture_in_marker_context(element, &flat_word, &left, &range, &selection_start, true)
        {
            Ok((capture, _, touched_edge)) => {
                let truncated = touched_edge
                    && matches!(&capture, SentenceCapture::Expanded { sentence, .. } if looks_truncated(sentence));
                log::info!(
                    "expanded the context by {lines} line(s) on each side, sentence still \
                     looks truncated: {truncated}"
                );
                let capture = match capture {
                    // 折叠空白会改写句子，原偏移作废（前端回退到按字符串匹配）
                    SentenceCapture::Expanded { sentence, .. } => SentenceCapture::Expanded {
                        sentence: collapse_whitespace(&sentence),
                        word_offset: None,
                    },
                    SentenceCapture::SelectionAsIs(text) => {
                        SentenceCapture::SelectionAsIs(collapse_whitespace(&text))
                    }
                };
                best = Some((capture, collapse_whitespace(&flat_word)));
                if !truncated {
                    break;
                }
            }
            Err(error) => {
                log::info!("capture with the {lines}-line context failed: {error}");
            }
        }
    }
    return match (best, first) {
        (Some(result), _) => Ok(result),
        // 扩展没有任何进展：沿用第一轮结果（可能是被截断的句子，或锚点失败的错误）
        (None, Ok((capture, _, _))) => Ok((capture, word.to_string())),
        (None, Err(error)) => Err(error),
    };
}

fn clone_capture(capture: &SentenceCapture) -> SentenceCapture {
    return match capture {
        SentenceCapture::Expanded {
            sentence,
            word_offset,
        } => SentenceCapture::Expanded {
            sentence: sentence.clone(),
            word_offset: *word_offset,
        },
        SentenceCapture::SelectionAsIs(text) => SentenceCapture::SelectionAsIs(text.clone()),
    };
}

/// 从 `from` 起沿 `attribute`（上一行行首 / 下一行行尾）走至多 `steps` 步；
/// 到头（报错或 marker 不再变化）即停，返回最后一个有效 marker。
fn step_lines(
    element: &core_foundation::base::CFType,
    attribute: &'static str,
    from: &core_foundation::base::CFType,
    steps: usize,
) -> core_foundation::base::CFType {
    use core_foundation::base::TCFType;

    let mut current = from.clone();
    for _ in 0..steps {
        match copy_parameterized_attribute_by_ax(element, attribute, current.as_CFTypeRef()) {
            Ok(next) if next != current => current = next,
            _ => break,
        }
    }
    return current;
}

/// 在 `context_range` 对应的文本上取句：偏移 = “上下文起点到选区起点”的前缀长度。
/// `flatten` 为 true 时先抹掉上下文里的换行（`word` 由调用方同步处理）。
/// 返回 {取句结果, 上下文是否含换行, 句子是否触边}。
fn capture_in_marker_context(
    element: &core_foundation::base::CFType,
    word: &str,
    context_start: &core_foundation::base::CFType,
    context_range: &core_foundation::base::CFType,
    selection_start: &core_foundation::base::CFType,
    flatten: bool,
) -> Result<(SentenceCapture, bool, bool), String> {
    use core_foundation::array::CFArray;
    use core_foundation::base::TCFType;

    /// 上下文的 UTF-16 码元上限：防御病态页面（无块级分隔的巨型文本节点），
    /// 超限降级为仅录词，不为切一句话读取整页文本
    const MAX_CONTEXT_UTF16_UNITS: usize = 50_000;

    let raw_context = string_for_marker_range_by_ax(element, context_range)?;
    let context_units = raw_context.encode_utf16().count();
    if context_units > MAX_CONTEXT_UTF16_UNITS {
        return Err(format!(
            "the context is too long ({context_units} utf16 units), skipping sentence extraction"
        ));
    }
    let markers = CFArray::from_CFTypes(&[context_start.clone(), selection_start.clone()]);
    let prefix = copy_parameterized_attribute_by_ax(
        element,
        ax_ffi::RANGE_FOR_UNORDERED_MARKERS_PARAMETERIZED_ATTRIBUTE,
        markers.as_CFTypeRef(),
    )?;
    let sel_start_utf16 = string_for_marker_range_by_ax(element, &prefix)?
        .encode_utf16()
        .count() as isize;
    let has_line_break = raw_context.contains('\n');
    let context = if flatten {
        flatten_line_breaks(&raw_context)
    } else {
        raw_context
    };

    // 偏移是本地推导的，与 Windows 同理不用“唯一出现位置”兜底（见 capture::find_anchor）。
    // window_loc 传 1：上下文起点不是文本开头，句子贴着起点同样算触边
    return match capture_in_context(&context, word, &[sel_start_utf16], 1, false) {
        Some((capture, touched_edge)) => Ok((capture, has_line_break, touched_edge)),
        None => Err(format!(
            "no validated anchor for the selected text in the context \
             (selection starts at {sel_start_utf16} utf16 units, context {context_units} utf16 \
             units, word {} utf16 units, {} occurrence(s) in the context; \
             a selection spanning several paragraphs lands here too)",
            word.encode_utf16().count(),
            context.match_indices(word).count()
        )),
    };
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
            let window =
                super::copy_element_attribute_by_ax(&app, super::ax_ffi::FOCUSED_WINDOW_ATTRIBUTE)
                    .expect("no focused window");
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

    /// 抹换行后，视觉行硬换行不再把跨行整句切碎；偏移（UTF-16）与长度保持不变
    #[test]
    fn flattened_pdf_lines_form_one_sentence() {
        use super::{capture_in_context, flatten_line_breaks, SentenceCapture};
        let raw = "3.2 Input Layer. The first thing to build a model is to\r\nprepare inputs with abundant information, such as User features\r\ninclude user ID and other user information. Next one.";
        let flat = flatten_line_breaks(raw);
        assert_eq!(flat.encode_utf16().count(), raw.encode_utf16().count());
        let offset = raw
            .find("abundant")
            .map(|byte| raw[..byte].encode_utf16().count())
            .unwrap();
        // 不抹换行：句子被换行切碎，只剩 "prepare inputs ... features"
        let (broken, _) =
            capture_in_context(raw, "abundant", &[offset as isize], 1, false).unwrap();
        match broken {
            SentenceCapture::Expanded { sentence, .. } => {
                assert!(!sentence.contains("The first thing"), "{sentence:?}")
            }
            _ => panic!("expected Expanded"),
        }
        let (capture, _) =
            capture_in_context(&flat, "abundant", &[offset as isize], 1, false).unwrap();
        match capture {
            SentenceCapture::Expanded { sentence, .. } => assert_eq!(
                sentence,
                "The first thing to build a model is to  prepare inputs with abundant information, such as User features  include user ID and other user information."
            ),
            _ => panic!("expected Expanded"),
        }
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
