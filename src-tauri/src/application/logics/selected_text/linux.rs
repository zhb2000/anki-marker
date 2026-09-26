//! Linux 取词实现：AT-SPI（D-Bus）优先，失败回退读 PRIMARY 选区。
//!
//! AT-SPI2 是 GTK/Qt/Chromium 共用的无障碍总线（走 D-Bus，Wayland 下不受影响；
//! 但全局快捷键本身仅在 X11 会话可用——global-hotkey 依赖 X11）。Qt 应用可能
//! 需要 QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1 才暴露无障碍树。
//!
//! 回退读 PRIMARY 选区：Linux 上选中文本天然进入 PRIMARY 剪贴板，只读不改、
//! 无需模拟按键；代价是没有选区偏移信息，只能录词、不能取句。
//!（GNOME Wayland 不支持 wlr-data-control 协议，此回退在 GNOME Wayland 下不可用。）
//!
//! AT-SPI 没有系统级“焦点元素”查询，取词采用两段式（镜像 macOS 的
//! 焦点元素优先 + 树搜索）：
//! 1. 在活跃窗口（State::Active）内用 Collection.GetMatches 直接查带 Focused
//!    状态的对象，取其 Text 接口的非空选区；
//! 2. 无果时对活跃窗口子树做有界 DFS（深度 ≤ 30、节点 ≤ 500，同 macOS 预算）。
//!
//! 注意 AT-SPI 的文本偏移按字符（Unicode codepoint）计，与共享 capture 模块
//! 期望的 UTF-16 码元不同，经 char_offset_to_utf16 换算后再切句。

use super::capture::{capture_in_context, char_offset_to_utf16, SentenceCapture};
use super::SelectedContext;

use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::collection::CollectionProxy;
use atspi::proxy::text::TextProxy;
use atspi::{AccessibilityConnection, Interface, MatchType, ObjectMatchRule, ObjectRefOwned, SortOrder, State};

pub fn get_selected_text() -> Result<String, String> {
    match tauri::async_runtime::block_on(atspi_selected_context(false)) {
        Ok(context) if !context.text.trim().is_empty() => return Ok(context.text),
        Ok(_) => {
            // AT-SPI “成功”但为空：无法区分“真没选”与“应用不暴露选区”，回退 PRIMARY 再判一次
        }
        Err(error) => {
            log::warn!("AT-SPI path failed, falling back to the PRIMARY selection: {error}");
        }
    }
    return primary_selection_fallback();
}

/// word_to_sentence 为 true 时尝试“选词取句”：AT-SPI 全链路成功返回 {句子, 词}；
/// 拿到词但取句失败返回 {词原文, None}；AT-SPI 报错或词为空时回退读 PRIMARY 选区。
/// word_to_sentence 为 false 时完全等同 get_selected_text 的行为。
///
/// `_on_retry_captured`：Linux 侧暂不做后台重试（AT-SPI 应用树一般即时物化，
/// 不像 macOS Word 那样有桩树窗口期），保留形参保持三端签名一致。
pub fn get_selected_context(
    word_to_sentence: bool,
    _on_retry_captured: impl FnOnce(SelectedContext) + Send + 'static,
) -> Result<SelectedContext, String> {
    if !word_to_sentence {
        return get_selected_text().map(|text| SelectedContext { text, word: None });
    }
    match tauri::async_runtime::block_on(atspi_selected_context(true)) {
        Ok(context) if !context.text.trim().is_empty() => return Ok(context),
        Ok(_) => {
            log::warn!(
                "AT-SPI path returned empty selected text (no selection or the app does not \
                 expose its selection via AT-SPI), falling back to the PRIMARY selection"
            );
        }
        Err(error) => {
            log::warn!("AT-SPI path failed, falling back to the PRIMARY selection: {error}");
        }
    }
    return primary_selection_fallback().map(|text| SelectedContext { text, word: None });
}

/// 回退：读 PRIMARY 选区（Linux 上选中文本天然进入 PRIMARY，只读不改剪贴板）。
fn primary_selection_fallback() -> Result<String, String> {
    use arboard::{GetExtLinux, LinuxClipboardKind};

    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("failed to access the clipboard: {error}"))?;
    return clipboard
        .get()
        .clipboard(LinuxClipboardKind::Primary)
        .text()
        .map_err(|error| format!("failed to read the PRIMARY selection: {error}"));
}

/// AT-SPI 取词取句链路：找到持有选区的 Text 对象 → 读所选词 →
///（word_to_sentence 时）窗口化读上下文 → 字符偏移换算 → 共享 capture 切句。
async fn atspi_selected_context(word_to_sentence: bool) -> Result<SelectedContext, String> {
    let connection = AccessibilityConnection::new()
        .await
        .map_err(|error| format!("failed to connect to the AT-SPI bus: {error}"))?;
    let selection = find_selection(&connection).await?;
    let word = selection
        .text
        .get_text(selection.start, selection.end)
        .await
        .map_err(|error| format!("failed to read the selected text: {error}"))?;
    if word.trim().is_empty() {
        return Ok(SelectedContext { text: word, word: None });
    }
    if !word_to_sentence {
        return Ok(SelectedContext { text: word, word: None });
    }
    return Ok(match find_sentence_for_word(&selection, &word).await {
        Ok(SentenceCapture::Expanded(sentence)) => SelectedContext {
            text: sentence,
            word: Some(word.trim().to_string()),
        },
        Ok(SentenceCapture::SelectionAsIs(text)) => SelectedContext { text, word: None },
        Err(error) => {
            // 已拿到词：取句失败不致命，降级为仅录入词原文
            log::warn!("failed to extract sentence for the selected word: {error}");
            SelectedContext { text: word, word: None }
        }
    });
}

/// 持有选区的 Text 对象及其选区（字符偏移，[start, end)）。
/// 代理的 destination/path 均为克隆的 owned 值，故生命周期为 'static。
struct AtspiSelection {
    text: TextProxy<'static>,
    start: i32,
    end: i32,
}

/// 在无障碍树中找到当前持有非空选区的 Text 对象：
/// 逐应用查活跃窗口（State::Active），先在窗口内按 Focused 状态精确匹配，
/// 无果再做有界 DFS。只搜活跃窗口既缩小了开销，也避免从其他应用的窗口里
/// 搜出陈旧选区（如本应用自己编辑框里的上一次选区）。
async fn find_selection(connection: &AccessibilityConnection) -> Result<AtspiSelection, String> {
    let root = connection
        .root_accessible_on_registry()
        .await
        .map_err(|error| format!("failed to get the registry root: {error}"))?;
    let apps = root
        .get_children()
        .await
        .map_err(|error| format!("failed to list accessible applications: {error}"))?;

    for app in &apps {
        let app_accessible = match accessible_from_ref(connection, app).await {
            Ok(accessible) => accessible,
            Err(_) => continue, // 应用可能已退出，跳过
        };
        let windows = match app_accessible.get_children().await {
            Ok(children) => children,
            Err(_) => continue,
        };
        for window_ref in &windows {
            let window = match accessible_from_ref(connection, window_ref).await {
                Ok(accessible) => accessible,
                Err(_) => continue,
            };
            let states = match window.get_state().await {
                Ok(states) => states,
                Err(_) => continue,
            };
            if !states.contains(State::Active) {
                continue; // 只搜活跃窗口（前台窗口）
            }
            if let Some(selection) = find_selection_in_window(connection, window_ref).await {
                return Ok(selection);
            }
        }
    }
    return Err("no active window holds a text selection via AT-SPI".to_string());
}

/// 在活跃窗口内找选区：先 Collection.GetMatches 精确匹配（Focused + Text 接口），
/// 无果再有界 DFS。
async fn find_selection_in_window(
    connection: &AccessibilityConnection,
    window_ref: &ObjectRefOwned,
) -> Option<AtspiSelection> {
    // 1) 精确匹配：带 Focused 状态且实现 Text 接口的对象（焦点控件即选区持有者，
    //    覆盖绝大多数场景，一次调用免去遍历整棵树）
    if let Ok(collection) = collection_from_ref(connection, window_ref).await {
        let rule = ObjectMatchRule::builder()
            .states([State::Focused], MatchType::All)
            .interfaces([Interface::Text], MatchType::All)
            .build();
        if let Ok(matches) = collection.get_matches(rule, SortOrder::Canonical, 8, false).await {
            for object in &matches {
                if let Some(selection) = selection_on_object(connection, object).await {
                    return Some(selection);
                }
            }
        }
    }
    // 2) 有界 DFS（显式栈，避免递归 async）：找第一个持有非空选区的 Text 对象
    let mut budget = MAX_TREE_NODES;
    let mut stack = vec![(window_ref.clone(), 0usize)];
    while let Some((object_ref, depth)) = stack.pop() {
        if depth > MAX_TREE_DEPTH || budget == 0 {
            return None;
        }
        budget -= 1;
        if let Some(selection) = selection_on_object(connection, &object_ref).await {
            return Some(selection);
        }
        if let Ok(accessible) = accessible_from_ref(connection, &object_ref).await {
            if let Ok(children) = accessible.get_children().await {
                // 逆序压栈保持先序遍历顺序
                for child in children.into_iter().rev() {
                    stack.push((child, depth + 1));
                }
            }
        }
    }
    return None;
}

/// DFS 搜索预算：深度与节点数上限（每次调用都是一次 D-Bus 往返，同 macOS 预算）
const MAX_TREE_DEPTH: usize = 30;
const MAX_TREE_NODES: usize = 500;

/// 若对象实现 Text 接口且持有非空选区，返回其 Text 代理与选区偏移。
async fn selection_on_object(
    connection: &AccessibilityConnection,
    object_ref: &ObjectRefOwned,
) -> Option<AtspiSelection> {
    let text = text_from_ref(connection, object_ref).await.ok()?;
    let count = text.get_n_selections().await.ok()?;
    if count <= 0 {
        return None;
    }
    let (start, end) = text.get_selection(0).await.ok()?;
    if end <= start {
        return None;
    }
    return Some(AtspiSelection { text, start, end });
}

/// 上下文窗口的单侧余量（字符数）。
const WINDOW_MARGIN: i32 = 1024;

/// 取句链路：窗口化读选区周边文本（±1024 字符，两端夹到文本边界）→
/// 字符偏移换算 UTF-16 → 共享 capture 校验锚点并切句。失败返回 Err 由调用方降级。
async fn find_sentence_for_word(
    selection: &AtspiSelection,
    word: &str,
) -> Result<SentenceCapture, String> {
    // get_text 的偏移越界是未定义行为（atspi 文档警告），必须先按字符数夹取
    let char_count = selection
        .text
        .character_count()
        .await
        .map_err(|error| format!("failed to get the character count: {error}"))?;
    if selection.start < 0 || selection.end > char_count {
        return Err(format!(
            "selection [{}, {}) is out of the text bounds ({} characters)",
            selection.start, selection.end, char_count
        ));
    }
    let window_start = std::cmp::max(0, selection.start - WINDOW_MARGIN);
    let window_end = std::cmp::min(char_count, selection.end + WINDOW_MARGIN);
    let context = selection
        .text
        .get_text(window_start, window_end)
        .await
        .map_err(|error| format!("failed to read the context window: {error}"))?;

    // AT-SPI 偏移按字符计，换算为 UTF-16 码元偏移后再切句
    let sel_start_chars = (selection.start - window_start) as usize;
    let sel_start_utf16 =
        char_offset_to_utf16(&context, sel_start_chars).ok_or_else(|| {
            format!(
                "selection offset {sel_start_chars} is out of the context ({} characters)",
                context.chars().count()
            )
        })? as isize;
    // window_loc：窗口起点的文档级偏移（0 表示已对齐文本开头），供触边判定
    match capture_in_context(&context, word, &[sel_start_utf16], window_start as isize) {
        Some((capture, touched_edge)) => {
            if touched_edge {
                log::info!(
                    "sentence touches the edge of a ±{WINDOW_MARGIN}-character context window \
                     (context {} utf16 units), accepting a possibly truncated sentence",
                    context.encode_utf16().count()
                );
            }
            return Ok(capture);
        }
        None => {
            return Err(format!(
                "no validated anchor for the selected text in the context \
                 (selection starts at {} utf16 units, context {} utf16 units, \
                 word {} utf16 units, {} occurrence(s) in context)",
                sel_start_utf16,
                context.encode_utf16().count(),
                word.encode_utf16().count(),
                context.match_indices(word).count()
            ));
        }
    }
}

/// 由对象引用构建 AccessibleProxy（destination/path 克隆为 owned，代理为 'static）。
async fn accessible_from_ref(
    connection: &AccessibilityConnection,
    object_ref: &ObjectRefOwned,
) -> Result<AccessibleProxy<'static>, String> {
    let name = object_ref.name().ok_or("the object reference is null")?;
    return AccessibleProxy::builder(connection.connection())
        .destination(name.clone())
        .map_err(|error| format!("invalid bus name: {error}"))?
        .path(object_ref.path().clone())
        .map_err(|error| format!("invalid object path: {error}"))?
        .build()
        .await
        .map_err(|error| format!("failed to build an Accessible proxy: {error}"));
}

/// 由对象引用构建 TextProxy（对象未实现 Text 接口时，后续方法调用会报错）。
async fn text_from_ref(
    connection: &AccessibilityConnection,
    object_ref: &ObjectRefOwned,
) -> Result<TextProxy<'static>, String> {
    let name = object_ref.name().ok_or("the object reference is null")?;
    return TextProxy::builder(connection.connection())
        .destination(name.clone())
        .map_err(|error| format!("invalid bus name: {error}"))?
        .path(object_ref.path().clone())
        .map_err(|error| format!("invalid object path: {error}"))?
        .build()
        .await
        .map_err(|error| format!("failed to build a Text proxy: {error}"));
}

/// 由对象引用构建 CollectionProxy。
async fn collection_from_ref(
    connection: &AccessibilityConnection,
    object_ref: &ObjectRefOwned,
) -> Result<CollectionProxy<'static>, String> {
    let name = object_ref.name().ok_or("the object reference is null")?;
    return CollectionProxy::builder(connection.connection())
        .destination(name.clone())
        .map_err(|error| format!("invalid bus name: {error}"))?
        .path(object_ref.path().clone())
        .map_err(|error| format!("invalid object path: {error}"))?
        .build()
        .await
        .map_err(|error| format!("failed to build a Collection proxy: {error}"));
}

#[cfg(test)]
mod tests {
    /// 冒烟测试：AT-SPI 链路不应 panic；无桌面环境（CI/容器）下允许返回 Err。
    #[test]
    fn atspi_smoke_test() {
        let result = tauri::async_runtime::block_on(super::atspi_selected_context(false));
        eprintln!(
            "atspi_selected_context() -> {:?}",
            result.map(|context| context.text.chars().count())
        );
    }
}
