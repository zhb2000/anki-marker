//! PDFKit 文档（Preview、Safari 的 PDF 查看器）的取句：选区位置不可得，
//! 经整页文本匹配定位选中文本再切句。

use super::super::capture::{capture_in_context, SentenceCapture};
use super::super::SelectedContext;
use super::ax::{
    copy_children_array_by_ax, copy_element_attribute_by_ax, copy_i64_attribute_by_ax,
    copy_parameterized_attribute_by_ax, copy_string_attribute_by_ax, create_application_element,
};
use super::ax_ffi;

/// 在窗口 AX 子树里找 PDFKit 页面（有界 DFS：深度、节点数都限制；Safari 的页面在
/// 深度 6 左右）。找到第一个 AXPage 即返回其全部兄弟页面。
fn find_pdf_pages_in_window(
    window: &core_foundation::base::CFType,
) -> Option<Vec<core_foundation::base::CFType>> {
    const MAX_DEPTH: usize = 10;
    const MAX_NODES: usize = 400;

    fn dfs(
        element: &core_foundation::base::CFType,
        depth: usize,
        budget: &mut usize,
    ) -> Option<Vec<core_foundation::base::CFType>> {
        if depth > MAX_DEPTH || *budget == 0 {
            return None;
        }
        *budget -= 1;
        let children = children_of(element);
        let pages: Vec<_> = children.iter().filter(|c| is_ax_page(c)).cloned().collect();
        if !pages.is_empty() {
            return Some(pages);
        }
        for child in &children {
            if let Some(found) = dfs(child, depth + 1, budget) {
                return Some(found);
            }
        }
        return None;
    }
    let mut budget = MAX_NODES;
    return dfs(window, 0, &mut budget);
}

/// 模拟复制只拿到了词（AX 完全不暴露选区）时，用目标应用窗口里的 PDF 页面文本补全句子。
///
/// Safari 的 PDF 查看器就是这种：连焦点元素都取不到，页面元素也不报 AXSelectedText，
/// 选区只能靠 Cmd+C 读到；但它的页面元素仍提供整页文本，词可据此定位（见
/// find_sentence_in_pdf_pages）。目标不是 PDF 或定位不唯一时返回 None，保持仅录词。
pub(super) fn refine_with_pdf_pages(pid: i32, selection: &str) -> Option<SelectedContext> {
    let started = std::time::Instant::now();
    let app = create_application_element(pid)?;
    let window = copy_element_attribute_by_ax(&app, ax_ffi::FOCUSED_WINDOW_ATTRIBUTE).ok()?;
    let Some(pages) = find_pdf_pages_in_window(&window) else {
        log::info!(
            "no PDF pages in the front window of pid {pid} ({:?}), keeping the word only",
            started.elapsed()
        );
        return None;
    };
    return match locate_sentence_in_pdf_pages(&pages, selection) {
        Ok(context) => {
            log::info!("PDF page text lookup succeeded ({:?})", started.elapsed());
            Some(context)
        }
        Err(error) => {
            log::warn!("PDF page text lookup failed: {error}");
            None
        }
    };
}

/// 屏幕坐标下的矩形（与 CGRect 内存布局一致，供 AXValueGetValue 解包 AXFrame）。
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
struct AxRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl AxRect {
    /// 与另一矩形的相交面积是否为正（仅相接不算）。
    fn intersects(&self, other: &AxRect) -> bool {
        let overlap_width = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
        let overlap_height =
            (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);
        return overlap_width > 0.0 && overlap_height > 0.0;
    }
}

/// 读取元素的屏幕外框（AXFrame）。
fn element_frame_by_ax(element: &core_foundation::base::CFType) -> Option<AxRect> {
    use core_foundation::base::TCFType;

    let value = copy_element_attribute_by_ax(element, ax_ffi::FRAME_ATTRIBUTE).ok()?;
    let mut rect = AxRect::default();
    let decoded = unsafe {
        ax_ffi::AXValueGetValue(
            value.as_CFTypeRef(),
            ax_ffi::AX_VALUE_CG_RECT_TYPE,
            &mut rect as *mut AxRect as *mut std::ffi::c_void,
        )
    };
    return (decoded != 0).then_some(rect);
}

/// 元素的子元素列表（各自 retain，不受数组生命周期约束）；读取失败返回空。
fn children_of(element: &core_foundation::base::CFType) -> Vec<core_foundation::base::CFType> {
    use core_foundation::base::{CFType, TCFType};

    let Ok(children) = copy_children_array_by_ax(element.as_CFTypeRef()) else {
        return Vec::new();
    };
    let count = unsafe { ax_ffi::CFArrayGetCount(children.as_CFTypeRef()) };
    let mut result = Vec::new();
    for index in 0..count {
        let raw = unsafe { ax_ffi::CFArrayGetValueAtIndex(children.as_CFTypeRef(), index) };
        if !raw.is_null() {
            let child: CFType = unsafe { TCFType::wrap_under_get_rule(raw) };
            result.push(child);
        }
    }
    return result;
}

fn is_ax_page(element: &core_foundation::base::CFType) -> bool {
    return copy_string_attribute_by_ax(element, ax_ffi::ROLE_ATTRIBUTE)
        .is_ok_and(|role| role == "AXPage");
}

/// 找出 PDFKit 文档的全部页面元素（AXPage 兄弟节点）。
///
/// 入口元素可能是页面本身或其后代（Safari：窗口树搜索命中的页面元素）→ 沿 AXParent
/// 上溯找到 AXPage，取其父节点下的全部 AXPage；也可能是页面的容器（Preview：焦点元素
/// 是 AXSharedDocumentContainer）→ 向下做有界广度搜索。找不到返回 None（不是 PDFKit 文档）。
fn find_pdf_pages(
    element: &core_foundation::base::CFType,
) -> Option<Vec<core_foundation::base::CFType>> {
    let pages_of = |parent: &core_foundation::base::CFType| -> Vec<core_foundation::base::CFType> {
        children_of(parent).into_iter().filter(is_ax_page).collect()
    };

    // 上溯：自身或祖先是 AXPage
    let mut current = element.clone();
    for _ in 0..12 {
        if is_ax_page(&current) {
            let parent = copy_element_attribute_by_ax(&current, ax_ffi::PARENT_ATTRIBUTE).ok()?;
            let pages = pages_of(&parent);
            return (!pages.is_empty()).then_some(pages);
        }
        match copy_element_attribute_by_ax(&current, ax_ffi::PARENT_ATTRIBUTE) {
            Ok(parent) => current = parent,
            Err(_) => break,
        }
    }

    // 下探：有界广度优先（层数、节点数都很小，PDF 文档容器紧挨着页面）
    const MAX_DEPTH: usize = 3;
    const MAX_NODES: usize = 80;
    let mut level = vec![element.clone()];
    let mut visited = 0;
    for _ in 0..MAX_DEPTH {
        let mut next_level = Vec::new();
        for node in &level {
            visited += 1;
            if visited > MAX_NODES {
                return None;
            }
            let children = children_of(node);
            let pages: Vec<_> = children.iter().filter(|c| is_ax_page(c)).cloned().collect();
            if !pages.is_empty() {
                return Some(pages);
            }
            next_level.extend(children);
        }
        level = next_level;
    }
    return None;
}

/// PDF 页面的可视区域：页面容器的最近 AXScrollArea 祖先（Preview），没有则取容器
/// 自身的外框（Safari 的 PDF 插件容器恰好就是视口）。
fn pdf_viewport(first_page: &core_foundation::base::CFType) -> Option<AxRect> {
    let container = copy_element_attribute_by_ax(first_page, ax_ffi::PARENT_ATTRIBUTE).ok()?;
    let mut current = container.clone();
    for _ in 0..8 {
        if copy_string_attribute_by_ax(&current, ax_ffi::ROLE_ATTRIBUTE)
            .is_ok_and(|role| role == "AXScrollArea")
        {
            return element_frame_by_ax(&current);
        }
        match copy_element_attribute_by_ax(&current, ax_ffi::PARENT_ATTRIBUTE) {
            Ok(parent) => current = parent,
            Err(_) => break,
        }
    }
    return element_frame_by_ax(&container);
}

/// 读取 PDF 页面的整页文本（UTF-16 码元序列）：AXAttributedStringForRange 取 [0, 字符数)。
fn pdf_page_text_utf16(page: &core_foundation::base::CFType) -> Result<Vec<u16>, String> {
    use core_foundation::base::{CFGetTypeID, CFRange, CFType, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    let count = copy_i64_attribute_by_ax(page, ax_ffi::NUMBER_OF_CHARACTERS_ATTRIBUTE)?;
    if count <= 0 {
        return Err("the page has no text".to_string());
    }
    let range = CFRange {
        location: 0,
        length: count as isize,
    };
    unsafe {
        let range_value_ref = ax_ffi::AXValueCreate(
            ax_ffi::AX_VALUE_CF_RANGE_TYPE,
            &range as *const CFRange as *const std::ffi::c_void,
        );
        if range_value_ref.is_null() {
            return Err("failed to create AXValue for CFRange".to_string());
        }
        let range_value: CFType = TCFType::wrap_under_create_rule(range_value_ref);
        let attributed = copy_parameterized_attribute_by_ax(
            page,
            ax_ffi::ATTRIBUTED_STRING_FOR_RANGE_PARAMETERIZED_ATTRIBUTE,
            range_value.as_CFTypeRef(),
        )?;
        if CFGetTypeID(attributed.as_CFTypeRef()) != ax_ffi::CFAttributedStringGetTypeID() {
            return Err("the page text is not an attributed string".to_string());
        }
        // Get 规则：借用自 attributed，转成 owned CFString 后再用
        let string_ref = ax_ffi::CFAttributedStringGetString(attributed.as_CFTypeRef());
        if string_ref.is_null() {
            return Err("the attributed string has no string".to_string());
        }
        let string: CFString = TCFType::wrap_under_get_rule(string_ref as CFStringRef);
        return Ok(string.to_string().encode_utf16().collect());
    }
}

/// 在页面文本中找选中文本的全部出现位置（UTF-16 起止偏移），比较时忽略空白差异
/// （选区文本与页面文本在换行/空格上可能不同）。区分大小写；子串匹配（选区可能是
/// 半个词）——出现位置越多越容易判为歧义，这是保守方向。
fn find_selection_occurrences(page: &[u16], selection: &str) -> Vec<(usize, usize)> {
    let is_space = |unit: u16| char::from_u32(unit as u32).is_some_and(char::is_whitespace);
    let needle: Vec<u16> = selection
        .encode_utf16()
        .filter(|unit| !is_space(*unit))
        .collect();
    if needle.is_empty() {
        return Vec::new();
    }
    // 页面文本去空白后的序列及其到原偏移的映射
    let mut compact = Vec::with_capacity(page.len());
    let mut origin = Vec::with_capacity(page.len());
    for (index, unit) in page.iter().enumerate() {
        if !is_space(*unit) {
            compact.push(*unit);
            origin.push(index);
        }
    }
    let mut result = Vec::new();
    if compact.len() >= needle.len() {
        for start in 0..=(compact.len() - needle.len()) {
            if compact[start..start + needle.len()] == needle[..] {
                result.push((origin[start], origin[start + needle.len() - 1] + 1));
            }
        }
    }
    return result;
}

/// 以页面文本中的一处出现位置为锚点取句，返回 {句子/所选原文, 页面中的所选词}。
/// 只取出现位置前后各 CONTEXT_MARGIN 个码元的窗口，防止超长页面拖慢分句。
fn capture_pdf_occurrence(page: &[u16], start: usize, end: usize) -> Option<SelectedContext> {
    const CONTEXT_MARGIN: usize = 4096;
    let window_start = start.saturating_sub(CONTEXT_MARGIN);
    let window_end = (end + CONTEXT_MARGIN).min(page.len());
    let context = String::from_utf16_lossy(&page[window_start..window_end]);
    let word = String::from_utf16_lossy(&page[start..end]);
    // 偏移来自本地匹配，不用“唯一出现位置”兜底（见 capture::find_anchor）
    let (capture, _touched_edge) = capture_in_context(
        &context,
        &word,
        &[(start - window_start) as isize],
        window_start as isize,
        false,
    )?;
    return Some(match capture {
        SentenceCapture::Expanded {
            sentence,
            word_offset,
        } => SelectedContext {
            text: sentence,
            word: Some(word.trim().to_string()),
            word_offset,
        },
        SentenceCapture::SelectionAsIs(text) => SelectedContext {
            text,
            word: None,
            word_offset: None,
        },
    });
}

/// PDFKit 文档（Preview、Safari 的 PDF 查看器）的选词取句。
///
/// PDFKit 的无障碍实现只给文档级的 AXSelectedText，不暴露选区位置（所有元素的
/// AXSelectedTextRange 都是占位值，AXBoundsForRange 也不可用）；但每个 AXPage 能经
/// AXAttributedStringForRange 给出整页文本。因此用“选中文本在页面文本中的出现位置”
/// 反推选区位置，按“宁可降级也不切错句”的原则只在能唯一确定时才取句：
/// 1. 先只看屏幕上可见的页面（选区必然在用户眼前）：恰好一处出现 → 定位成功；
/// 2. 可见页面里没有（选区已被滚出屏幕）→ 从可见页向两侧就近在其余页面（有上限）里找，
///    同样要求恰好一处；没有任何页面可判定为可见（视口未知）时不做这一步、直接降级——
///    此时无从判断用户在看哪里，文档别处的唯一出现很可能不是用户选的那一处；
/// 3. 多处出现：若各处切出的句子完全相同也接受，否则判歧义——高频词会降级为仅录词。
///
/// 已知的残余风险：选区被滚出屏幕、而可见页面里恰好有同一个词的唯一出现——会定位到
/// 可见页面里的那一处。
pub(super) fn find_sentence_in_pdf_pages(
    element: &core_foundation::base::CFType,
    word: &str,
) -> Result<SelectedContext, String> {
    let pages = find_pdf_pages(element).ok_or("not a PDFKit document (no AXPage found)")?;
    return locate_sentence_in_pdf_pages(&pages, word);
}

/// 可见页面里没找到选中文本时，接着要扫描的不可见页下标：离可见页近的优先（选区多半
/// 刚被滚出屏幕），最多 `limit` 页。不能按文档顺序取：用户在第 100 页选的词可能恰好在
/// 前 40 页有唯一出现，会被“自信地”定位到错误的句子。
///
/// 没有可见页（视口未知）返回 None：调用方应直接降级，见 find_sentence_in_pdf_pages。
fn hidden_pages_to_scan(visible: &[bool], limit: usize) -> Option<Vec<usize>> {
    let visible_pages: Vec<usize> = (0..visible.len()).filter(|i| visible[*i]).collect();
    if visible_pages.is_empty() {
        return None;
    }
    let mut hidden: Vec<usize> = (0..visible.len()).filter(|i| !visible[*i]).collect();
    // 稳定排序：距离相同时前面的页优先
    hidden.sort_by_key(|page| {
        visible_pages
            .iter()
            .map(|visible_page| page.abs_diff(*visible_page))
            .min()
    });
    hidden.truncate(limit);
    return Some(hidden);
}

/// find_sentence_in_pdf_pages 的定位部分：已知页面元素列表，按选中文本定位并取句。
fn locate_sentence_in_pdf_pages(
    pages: &[core_foundation::base::CFType],
    word: &str,
) -> Result<SelectedContext, String> {
    /// 第二阶段最多扫描的不可见页数（每页一次跨进程调用，页面文本可达上万字符）
    const MAX_HIDDEN_PAGES_SCANNED: usize = 40;

    let viewport = pdf_viewport(&pages[0]);
    let visible: Vec<bool> = pages
        .iter()
        .map(|page| match (&viewport, element_frame_by_ax(page)) {
            (Some(viewport), Some(frame)) => viewport.intersects(&frame),
            _ => false,
        })
        .collect();

    // 各页文本按需读取一次；candidates 为 (页下标, 起, 止)
    let mut texts: Vec<Option<Vec<u16>>> = vec![None; pages.len()];
    let mut candidates: Vec<(usize, usize, usize)> = Vec::new();
    let scan = |index: usize,
                texts: &mut Vec<Option<Vec<u16>>>,
                candidates: &mut Vec<(usize, usize, usize)>| {
        match pdf_page_text_utf16(&pages[index]) {
            Ok(text) => {
                for (start, end) in find_selection_occurrences(&text, word) {
                    candidates.push((index, start, end));
                }
                texts[index] = Some(text);
            }
            Err(error) => log::info!("PDF page {index}: cannot read the text: {error}"),
        }
    };

    let visible_count = visible.iter().filter(|v| **v).count();
    for index in (0..pages.len()).filter(|i| visible[*i]) {
        scan(index, &mut texts, &mut candidates);
    }
    let mut stage = "visible pages";
    if candidates.is_empty() {
        let Some(hidden) = hidden_pages_to_scan(&visible, MAX_HIDDEN_PAGES_SCANNED) else {
            return Err(format!(
                "none of the {} page(s) is known to be visible (unknown viewport), \
                 the selection cannot be located",
                pages.len()
            ));
        };
        stage = "other pages";
        for index in hidden {
            scan(index, &mut texts, &mut candidates);
        }
    }
    log::info!(
        "PDF sentence capture: {} page(s), {visible_count} visible, {} occurrence(s) of the \
         selection found in the {stage}",
        pages.len(),
        candidates.len()
    );

    if candidates.is_empty() {
        return Err(format!(
            "the selected text ({} utf16 units) was not found in the page text",
            word.encode_utf16().count()
        ));
    }
    let mut contexts = Vec::new();
    for (index, start, end) in &candidates {
        let text = texts[*index].as_ref().expect("scanned page has text");
        contexts.push(
            capture_pdf_occurrence(text, *start, *end)
                .ok_or("failed to cut a sentence at the located occurrence")?,
        );
    }
    let first = contexts.remove(0);
    if contexts
        .iter()
        .any(|other| other.text != first.text || other.word != first.word)
    {
        return Err(format!(
            "the selected text occurs {} times with different surrounding sentences, \
             the selection cannot be located",
            candidates.len()
        ));
    }
    return Ok(first);
}

#[cfg(test)]
mod tests {
    use super::super::ax::{find_element_with_selection_in_window, frontmost_application_pid};
    use super::super::context_from_word_and_element;

    /// 选中文本在页面文本中的定位：忽略空白差异、记录全部出现位置、UTF-16 偏移
    #[test]
    fn selection_occurrences_in_page_text() {
        let page: Vec<u16> = "Use abundant data. Abundant? An abun dant word, abundant"
            .encode_utf16()
            .collect();
        // 区分大小写；空白差异不影响匹配（"abun dant" 命中 "abundant"）
        let found = super::find_selection_occurrences(&page, "abundant");
        let text = String::from_utf16_lossy(&page);
        let slices: Vec<String> = found
            .iter()
            .map(|(start, end)| String::from_utf16_lossy(&page[*start..*end]))
            .collect();
        assert_eq!(
            slices,
            vec!["abundant", "abun dant", "abundant"],
            "{found:?} in {text:?}"
        );
        // 选区本身含换行（跨行选择）：与页面里的空格等价
        let page: Vec<u16> = "the quick brown fox".encode_utf16().collect();
        assert_eq!(
            super::find_selection_occurrences(&page, "quick\nbrown"),
            vec![(4, 15)]
        );
        // astral 字符：偏移按 UTF-16 码元
        let page: Vec<u16> = "say 😀 hi".encode_utf16().collect();
        assert_eq!(super::find_selection_occurrences(&page, "😀"), vec![(4, 6)]);
        assert!(super::find_selection_occurrences(&page, "  ").is_empty());
        assert!(super::find_selection_occurrences(&page, "zebra").is_empty());
    }

    /// 以出现位置为锚点取句：单句扩展、窗口偏移不影响结果
    #[test]
    fn pdf_occurrence_capture() {
        let page: Vec<u16> =
            "Intro line.\nThe first thing is to prepare inputs with abundant information, such as features. Next one."
                .encode_utf16()
                .collect();
        let (start, end) = super::find_selection_occurrences(&page, "abundant")[0];
        let context = super::capture_pdf_occurrence(&page, start, end).expect("should capture");
        assert_eq!(
            context.text,
            "The first thing is to prepare inputs with abundant information, such as features."
        );
        assert_eq!(context.word.as_deref(), Some("abundant"));
    }

    /// 实机诊断（需 AX_TEST_PID=目标应用 pid，其 PDF 窗口里已选中文字）：走“焦点应用窗口
    /// 搜索 → 取句”链路，打印结果与耗时。
    #[test]
    fn pdf_context_by_pid_diagnostic() {
        use core_foundation::base::{CFType, TCFType};
        let Some(pid) = std::env::var("AX_TEST_PID")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
        else {
            return;
        };
        let started = std::time::Instant::now();
        unsafe {
            let app: CFType =
                TCFType::wrap_under_create_rule(super::ax_ffi::AXUIElementCreateApplication(pid));
            let window =
                super::copy_element_attribute_by_ax(&app, super::ax_ffi::FOCUSED_WINDOW_ATTRIBUTE)
                    .expect("no focused window");
            let found = find_element_with_selection_in_window(&window);
            eprintln!(
                "window search: {:?} in {:?}",
                found.as_ref().map(|(_, w)| w),
                started.elapsed()
            );
            let (element, word) = found.expect("no element with a selection");
            let result = context_from_word_and_element(&element, &word);
            eprintln!(
                "context_from_word_and_element -> {result:?} (total {:?})",
                started.elapsed()
            );
            // 应用级焦点元素入口（Preview 的正常路径：焦点元素是页面容器，需向下找页面）
            if let Ok(focused) = super::copy_element_attribute_by_ax(
                &app,
                super::ax_ffi::FOCUSED_UI_ELEMENT_ATTRIBUTE,
            ) {
                let started = std::time::Instant::now();
                let word = super::copy_string_attribute_by_ax(
                    &focused,
                    super::ax_ffi::SELECTED_TEXT_ATTRIBUTE,
                )
                .unwrap_or_default();
                let result = context_from_word_and_element(&focused, &word);
                eprintln!(
                    "via focused element -> {result:?} ({:?})",
                    started.elapsed()
                );
            }
        }
    }

    /// 实机诊断（需 AX_TEST_PID 与 AX_TEST_WORD）：模拟“Cmd+C 只拿到词”后的 PDF 页面文本补全。
    #[test]
    fn pdf_refine_by_pid_diagnostic() {
        let (Some(pid), Ok(word)) = (
            std::env::var("AX_TEST_PID")
                .ok()
                .and_then(|v| v.parse::<i32>().ok()),
            std::env::var("AX_TEST_WORD"),
        ) else {
            return;
        };
        let started = std::time::Instant::now();
        let result = super::refine_with_pdf_pages(pid, &word);
        eprintln!(
            "refine_with_pdf_pages -> {result:?} ({:?})",
            started.elapsed()
        );
        eprintln!(
            "frontmost_application_pid -> {:?}",
            frontmost_application_pid()
        );
    }

    /// 第二阶段的扫描页：视口未知时不扫（直接降级）；否则从可见页向两侧就近、受上限约束
    #[test]
    fn hidden_page_scan_order() {
        // 没有可见页（含视口未知）：绝不按文档顺序扫前若干页
        assert_eq!(super::hidden_pages_to_scan(&[false; 5], 40), None);
        assert_eq!(super::hidden_pages_to_scan(&[], 40), None);
        // 就近：离可见页（下标 5）越近越先扫，距离相同时前面的页优先
        let mut visible = [false; 8];
        visible[5] = true;
        assert_eq!(
            super::hidden_pages_to_scan(&visible, 40),
            Some(vec![4, 6, 3, 7, 2, 1, 0])
        );
        // 上限截断的是远处的页：用户在文档后部时不会扫到文档开头
        assert_eq!(
            super::hidden_pages_to_scan(&visible, 3),
            Some(vec![4, 6, 3])
        );
        // 多个可见页：距离取到最近可见页的距离；全部可见时无需再扫
        assert_eq!(
            super::hidden_pages_to_scan(&[true, false, false, false, true], 40),
            Some(vec![1, 3, 2])
        );
        assert_eq!(
            super::hidden_pages_to_scan(&[true, true], 40),
            Some(Vec::new())
        );
    }
}
