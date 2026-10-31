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
//!
//! ## 超文本与 U+FFFC
//!
//! 浏览器的 Text 接口是分层的：父对象的文本里，每个内嵌子对象（链接、行内 code、
//! 图片、文档下的段落等）都只占一个 U+FFFC 占位符。不处理的话“Hello ￼ there.”
//! 这样的文本会一路录进卡片，而且锚点校验在同一份文本里做，拦不住。
//! 选区还会逐层上报给祖先对象，祖先报告的是子对象占位符的偏移（Firefox 的
//! HyperTextAccessibleBase::SelectionBoundsAt 如此，Chromium 未核实），焦点在文档
//! 上时读到的“词”可能只是段落对应的一个占位符。因此：
//! 1. 选区恰为一个块级子对象的占位符时，下沉到子对象（descend_embedded_selection）；
//! 2. 上下文里的占位符经 Hypertext/Hyperlink 展开成子对象文本（行内对象的选区仍留在
//!    父对象上，句子才不会被截成链接文字本身），偏移换算在 capture 模块；
//! 3. 占位符解析失败时不录缺字的文本：落进句子就降级为仅录词，落进词就回退 PRIMARY
//!    （PRIMARY 是选区的真实文本）。
//!
//! ## PRIMARY 的可信度
//!
//! X11 下取消选中后 PRIMARY 往往还留着旧内容，无条件回退会录入很久以前的选区。
//! 按 AT-SPI 的结论分两种情况（`LAST_PRIMARY` 记录上次读到的内容）：
//! - AT-SPI 搜完了活跃窗口、确实没有选区：有可能真没选，也可能应用不暴露选区，
//!   所以只有 PRIMARY 相对上次变化才采用，否则视为空；
//! - AT-SPI 不可用、超时或找不到活跃窗口：没有任何反证，PRIMARY 是唯一来源，照常采用。
//!
//! 已知代价：比较的是内容，无法读取 X11 的选区时间戳。在不暴露选区的应用里重复取
//! 同一段文字、或取消后重新选中相同文字，第二次会被判为空；应用启动后的第一次回退
//! 没有比较基准，仍会采用旧内容。
//!
//! ## 待办：托盘菜单触发时的取词目标（2026-09-28 记，尚未实机验证）
//!
//! Windows 上托盘菜单会抢走前台窗口（菜单由 tray-icon 持有，见 windows_target），
//! Linux 的结构相同——菜单由宿主面板（libappindicator / SNI）持有，从菜单项触发时
//! 活跃窗口（`State::Active`）不在用户所在的应用上，AT-SPI 两段式取词（活跃窗口内
//! 查 Focused 元素 / 树搜索）因此大概率取不到；回退读 PRIMARY 与焦点无关，仍能取到
//! 已选中的**词**，但拿不到选区偏移与上下文，即取不到句子。待办方向（与 Windows
//! 同构）：记录“用户最近所在的应用窗口”，在菜单触发的取词前把活动窗口还回去——
//! X11 下可用 EWMH（`_NET_ACTIVE_WINDOW` / `_NET_CLIENT_LIST_STACKING`）或
//! `_NET_ACTIVE_WINDOW` 客户端消息，Wayland 下没有等价物（另一个已接受的限制）。
//! 前提是先实机确认症状（Ubuntu 24 VM：菜单项取词是否只录到词、是否取不到句子）。

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use super::capture::{
    capture_in_context, capture_with_embedded_objects, char_offset_to_utf16, remember_primary,
    EmbeddedObject, SentenceCapture, EMBEDDED_OBJECT_CHAR,
};
use super::{RetryCallbacks, SelectedContext};

use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::collection::CollectionProxy;
use atspi::proxy::hyperlink::HyperlinkProxy;
use atspi::proxy::hypertext::HypertextProxy;
use atspi::proxy::text::TextProxy;
use atspi::{
    AccessibilityConnection, Interface, MatchType, ObjectMatchRule, ObjectRefOwned, Role,
    SortOrder, State,
};

/// AT-SPI 链路的总超时：卡死的应用不回复 D-Bus，不能让它拖住每一次取词。
const ATSPI_TIMEOUT: Duration = Duration::from_millis(1500);

pub fn get_selected_text() -> Result<String, String> {
    match atspi_context_with_timeout(false) {
        Ok(context) if !context.text.trim().is_empty() => {
            remember_primary_in_background();
            return Ok(context.text);
        }
        Ok(_) => {
            // AT-SPI “成功”但为空：无法区分“真没选”与“应用不暴露选区”，只采用有变化的 PRIMARY
            return Ok(changed_primary_selection());
        }
        Err(error) => {
            log::warn!("AT-SPI path failed, falling back to the PRIMARY selection: {error}");
        }
    }
    return primary_selection_fallback();
}

/// word_to_sentence 为 true 时尝试“选词取句”：AT-SPI 全链路成功返回 {句子, 词}；
/// 拿到词但取句失败返回 {词原文, None}；AT-SPI 报错或词为空时回退读 PRIMARY 选区
///（词为空时只采用有变化的 PRIMARY，见模块文档）。
/// word_to_sentence 为 false 时完全等同 get_selected_text 的行为。
///
/// `_callbacks`：Linux 侧暂不做后台重试（AT-SPI 应用树一般即时物化，
/// 不像 macOS Word 那样有桩树窗口期），保留形参保持三端签名一致。
pub fn get_selected_context(
    word_to_sentence: bool,
    _callbacks: RetryCallbacks,
) -> Result<SelectedContext, String> {
    if !word_to_sentence {
        return get_selected_text().map(|text| SelectedContext {
            text,
            word: None,
            word_offset: None,
        });
    }
    match atspi_context_with_timeout(true) {
        Ok(context) if !context.text.trim().is_empty() => {
            remember_primary_in_background();
            return Ok(context);
        }
        Ok(_) => {
            log::warn!(
                "AT-SPI path returned empty selected text (no selection or the app does not \
                 expose its selection via AT-SPI), checking the PRIMARY selection for a change"
            );
            return Ok(SelectedContext {
                text: changed_primary_selection(),
                word: None,
                word_offset: None,
            });
        }
        Err(error) => {
            log::warn!("AT-SPI path failed, falling back to the PRIMARY selection: {error}");
        }
    }
    return primary_selection_fallback().map(|text| SelectedContext {
        text,
        word: None,
        word_offset: None,
    });
}

/// 上次读到的 PRIMARY 内容，判断 PRIMARY 是否变化的基准（见模块文档）。
static LAST_PRIMARY: Mutex<Option<String>> = Mutex::new(None);

fn note_primary(text: &str) -> bool {
    let mut last = LAST_PRIMARY
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    return remember_primary(&mut last, text);
}

/// 回退：读 PRIMARY 选区并更新比较基准。AT-SPI 给不出任何结论时使用。
fn primary_selection_fallback() -> Result<String, String> {
    let text = read_primary_selection()?;
    note_primary(&text);
    return Ok(text);
}

/// AT-SPI 确认没有选区时读 PRIMARY：内容没变就是旧选区，返回空串。
/// 读取失败同样按空处理——AT-SPI 已正常响应，这里没有“取词出错”可报（比如 PRIMARY
/// 没有持有者，或 GNOME Wayland 下读不到）。
fn changed_primary_selection() -> String {
    return match read_primary_selection() {
        Ok(text) if note_primary(&text) => text,
        Ok(_) => {
            log::info!("the PRIMARY selection is unchanged, treating it as no selection");
            String::new()
        }
        Err(error) => {
            log::warn!("{error}");
            String::new()
        }
    };
}

/// AT-SPI 取到选区时也更新基准：否则用户取消选中再取词，这次已录入的内容会被当成
/// “变化了的 PRIMARY”再录一遍。放后台线程，因为 PRIMARY 持有者卡死时 arboard
/// 最多要等 4 秒，不能拖慢已经成功的取词。
fn remember_primary_in_background() {
    std::thread::spawn(|| {
        if let Ok(text) = read_primary_selection() {
            note_primary(&text);
        }
    });
}

/// 读 PRIMARY 选区（Linux 上选中文本天然进入 PRIMARY，只读不改剪贴板）。
fn read_primary_selection() -> Result<String, String> {
    use arboard::{GetExtLinux, LinuxClipboardKind};

    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| format!("failed to access the clipboard: {error}"))?;
    return clipboard
        .get()
        .clipboard(LinuxClipboardKind::Primary)
        .text()
        .map_err(|error| format!("failed to read the PRIMARY selection: {error}"));
}

/// 带总超时地跑 AT-SPI 链路。tauri 的异步运行时没有导出超时原语，又不想为此直接依赖
/// tokio，所以把链路放进任务、在调用线程上 recv_timeout（调用方本就是专用取词线程，
/// 可以阻塞）；超时后 abort 任务，取消进行中的 D-Bus 调用。
///
/// 超时但活跃窗口已搜完时，返回“没有选区”：说明卡住的是别的应用，结论已经有了，
/// 不能因此把 PRIMARY 当成无条件可信。
fn atspi_context_with_timeout(word_to_sentence: bool) -> Result<SelectedContext, String> {
    let active_window_searched = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    let task = tauri::async_runtime::spawn({
        let active_window_searched = active_window_searched.clone();
        async move {
            let result = atspi_selected_context(word_to_sentence, &active_window_searched).await;
            let _ = sender.send(result);
        }
    });
    return match receiver.recv_timeout(ATSPI_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            task.abort();
            if active_window_searched.load(Ordering::Relaxed) {
                log::warn!("AT-SPI timed out after the active window was searched");
                Ok(empty_context())
            } else {
                Err(format!("AT-SPI timed out after {ATSPI_TIMEOUT:?}"))
            }
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("the AT-SPI task ended without a result".to_string())
        }
    };
}

fn empty_context() -> SelectedContext {
    return SelectedContext {
        text: String::new(),
        word: None,
        word_offset: None,
    };
}

/// AT-SPI 取词取句链路：找到持有选区的 Text 对象 → 读所选词 →
///（word_to_sentence 时）窗口化读上下文 → 字符偏移换算 → 共享 capture 切句。
///
/// 返回 Ok（空文本）表示 AT-SPI 正常响应但没有选区，Err 表示 AT-SPI 给不出结论；
/// 两者的 PRIMARY 回退策略不同（见模块文档）。
async fn atspi_selected_context(
    word_to_sentence: bool,
    active_window_searched: &AtomicBool,
) -> Result<SelectedContext, String> {
    let connection = AccessibilityConnection::new()
        .await
        .map_err(|error| format!("failed to connect to the AT-SPI bus: {error}"))?;
    let Some(selection) = find_selection(&connection, active_window_searched).await? else {
        return Ok(empty_context());
    };
    let selection = descend_embedded_selection(&connection, selection).await;
    let word = selection
        .text
        .get_text(selection.start, selection.end)
        .await
        .map_err(|error| format!("failed to read the selected text: {error}"))?;
    if word.trim().is_empty() {
        return Ok(SelectedContext {
            text: word,
            word: None,
            word_offset: None,
        });
    }
    let word_has_object = word.contains(EMBEDDED_OBJECT_CHAR);
    if !word_to_sentence {
        if word_has_object {
            return Err(EMBEDDED_WORD_ERROR.to_string());
        }
        return Ok(SelectedContext {
            text: word,
            word: None,
            word_offset: None,
        });
    }
    let sentence_result = find_sentence_for_word(&connection, &selection, &word).await;
    return Ok(match sentence_result {
        Ok((
            SentenceCapture::Expanded {
                sentence,
                word_offset,
            },
            real_word,
        )) => SelectedContext {
            text: sentence,
            word: Some(real_word.trim().to_string()),
            word_offset,
        },
        // 跨段选区展开后段落间没有分隔，所选文本以 PRIMARY 为准
        Ok((SentenceCapture::SelectionAsIs(_), _)) if word_has_object => {
            return Err(EMBEDDED_WORD_ERROR.to_string());
        }
        Ok((SentenceCapture::SelectionAsIs(text), _)) => SelectedContext {
            text,
            word: None,
            word_offset: None,
        },
        Err(error) if word_has_object => {
            return Err(format!("{EMBEDDED_WORD_ERROR}: {error}"));
        }
        Err(error) => {
            // 已拿到词：取句失败不致命，降级为仅录入词原文
            log::warn!("failed to extract sentence for the selected word: {error}");
            SelectedContext {
                text: word,
                word: None,
                word_offset: None,
            }
        }
    });
}

/// 所选文本含内嵌对象占位符、无法还原时的错误：调用方据此回退 PRIMARY。
const EMBEDDED_WORD_ERROR: &str = "the selected text contains embedded objects";

/// 持有选区的 Text 对象及其选区（字符偏移，[start, end)）。
/// 代理的 destination/path 均为克隆的 owned 值，故生命周期为 'static。
struct AtspiSelection {
    object: ObjectRefOwned,
    text: TextProxy<'static>,
    start: i32,
    end: i32,
}

/// 任务句柄的守卫：find_selection 提前返回或随外层超时被取消时，
/// 一并取消还在等待卡死应用回复的查询任务，避免任务堆积。
struct AbortOnDrop(Vec<tauri::async_runtime::JoinHandle<()>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

/// 在无障碍树中找到当前持有非空选区的 Text 对象：
/// 并发查各应用的活跃窗口（State::Active），每个窗口内先按 Focused 状态精确匹配，
/// 无果再做有界 DFS。只搜活跃窗口既缩小了开销，也避免从其他应用的窗口里
/// 搜出陈旧选区（如本应用自己编辑框里的上一次选区）。
///
/// 返回 Ok(None) 表示活跃窗口都搜过了、没有选区；一个活跃窗口都没找到时返回 Err。
/// 并发是为了让卡死的应用只拖住它自己：最先回复的活跃窗口立即开搜，不必排队等它。
async fn find_selection(
    connection: &AccessibilityConnection,
    active_window_searched: &AtomicBool,
) -> Result<Option<AtspiSelection>, String> {
    let root = connection
        .root_accessible_on_registry()
        .await
        .map_err(|error| format!("failed to get the registry root: {error}"))?;
    let apps = root
        .get_children()
        .await
        .map_err(|error| format!("failed to list accessible applications: {error}"))?;

    // 容量足够所有应用各发一次，搜窗口期间查询任务不会阻塞在发送上
    let (sender, mut receiver) = tauri::async_runtime::channel(apps.len().max(1));
    let mut tasks = AbortOnDrop(Vec::with_capacity(apps.len()));
    for app in apps {
        let connection = connection.clone();
        let sender = sender.clone();
        tasks.0.push(tauri::async_runtime::spawn(async move {
            let _ = sender
                .send(active_windows_of_app(&connection, &app).await)
                .await;
        }));
    }
    drop(sender);

    let mut found_active_window = false;
    while let Some(windows) = receiver.recv().await {
        for window_ref in &windows {
            found_active_window = true;
            if let Some(selection) = find_selection_in_window(connection, window_ref).await {
                return Ok(Some(selection));
            }
            active_window_searched.store(true, Ordering::Relaxed);
        }
    }
    if found_active_window {
        return Ok(None);
    }
    return Err("no active window was found via AT-SPI".to_string());
}

/// 列出应用的活跃窗口。应用可能已退出，查询失败的应用或窗口直接跳过。
async fn active_windows_of_app(
    connection: &AccessibilityConnection,
    app: &ObjectRefOwned,
) -> Vec<ObjectRefOwned> {
    let mut active_windows = Vec::new();
    let app_accessible = match accessible_from_ref(connection, app).await {
        Ok(accessible) => accessible,
        Err(_) => return active_windows,
    };
    let windows = match app_accessible.get_children().await {
        Ok(children) => children,
        Err(_) => return active_windows,
    };
    for window_ref in windows {
        let window = match accessible_from_ref(connection, &window_ref).await {
            Ok(accessible) => accessible,
            Err(_) => continue,
        };
        let states = match window.get_state().await {
            Ok(states) => states,
            Err(_) => continue,
        };
        if states.contains(State::Active) {
            active_windows.push(window_ref); // 只搜活跃窗口（前台窗口）
        }
    }
    return active_windows;
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
        if let Ok(matches) = collection
            .get_matches(rule, SortOrder::Canonical, 8, false)
            .await
        {
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
    return Some(AtspiSelection {
        object: object_ref.clone(),
        text,
        start,
        end,
    });
}

/// 内嵌对象的递归深度：引用角标是“段落 → sup → 链接”，留出一些余量。
const MAX_EMBEDDED_DEPTH: usize = 6;
/// 单次取句最多解析的内嵌对象数：每个对象要多次 D-Bus 往返，且只有选区附近的才可能
/// 进入句子，由近及远解析，用完后剩下的占位符按未解析处理。
const MAX_EMBEDDED_OBJECTS: usize = 64;

/// 选区在祖先对象上只能精确到占位符：恰为一个块级子对象的占位符时下沉到子对象，
/// 否则文档层面读到的“词”和“上下文”全是占位符。行内对象不下沉：它的文本只是句子
/// 的一段，下沉后上下文就只剩链接文字，整句都取不到。
async fn descend_embedded_selection(
    connection: &AccessibilityConnection,
    mut selection: AtspiSelection,
) -> AtspiSelection {
    for _ in 0..MAX_EMBEDDED_DEPTH {
        if selection.end - selection.start != 1 {
            break;
        }
        match selection
            .text
            .get_text(selection.start, selection.end)
            .await
        {
            Ok(text) if text.chars().eq([EMBEDDED_OBJECT_CHAR]) => {}
            _ => break,
        }
        let Ok(hypertext) = hypertext_from_ref(connection, &selection.object).await else {
            break;
        };
        let Some(child) = embedded_object_at(connection, &hypertext, selection.start).await else {
            break;
        };
        if is_inline_object(connection, &child).await {
            break;
        }
        match selection_on_object(connection, &child).await {
            Some(child_selection) => selection = child_selection,
            None => break,
        }
    }
    return selection;
}

/// 对象是否为句内的行内元素。查询失败时按行内处理：不下沉顶多取不到句子，
/// 下沉错了却可能把句子截成一个片段。
async fn is_inline_object(
    connection: &AccessibilityConnection,
    object_ref: &ObjectRefOwned,
) -> bool {
    let Ok(accessible) = accessible_from_ref(connection, object_ref).await else {
        return true;
    };
    let Ok(role) = accessible.get_role().await else {
        return true;
    };
    return matches!(
        role,
        Role::Link
            | Role::Static
            | Role::Subscript
            | Role::Superscript
            | Role::Mark
            | Role::ContentDeletion
            | Role::ContentInsertion
            | Role::Suggestion
    );
}

/// 取父对象文本里 `char_index` 处占位符对应的子对象。
async fn embedded_object_at(
    connection: &AccessibilityConnection,
    hypertext: &HypertextProxy<'static>,
    char_index: i32,
) -> Option<ObjectRefOwned> {
    let link_index = hypertext.get_link_index(char_index).await.ok()?;
    if link_index < 0 {
        return None;
    }
    let link_ref = hypertext.get_link(link_index).await.ok()?;
    let link = hyperlink_from_ref(connection, &link_ref).await.ok()?;
    return link.get_object(0).await.ok();
}

/// 解析上下文窗口里的全部占位符，结果与占位符按出现顺序一一对应，解析不了的为 None。
async fn resolve_embedded_objects(
    connection: &AccessibilityConnection,
    selection: &AtspiSelection,
    context: &str,
    window_start: i32,
) -> Vec<Option<EmbeddedObject>> {
    let positions: Vec<usize> = context
        .chars()
        .enumerate()
        .filter(|(_, ch)| *ch == EMBEDDED_OBJECT_CHAR)
        .map(|(index, _)| index)
        .collect();
    let mut objects = vec![None; positions.len()];
    let Ok(hypertext) = hypertext_from_ref(connection, &selection.object).await else {
        return objects;
    };
    let sel_start = (selection.start - window_start) as usize;
    let sel_end = (selection.end - window_start) as usize;
    let mut nearest_first: Vec<usize> = (0..positions.len()).collect();
    nearest_first.sort_by_key(|&index| positions[index].abs_diff(sel_start));
    let mut budget = MAX_EMBEDDED_OBJECTS;
    for index in nearest_first {
        if budget == 0 {
            break;
        }
        let position = positions[index];
        let char_index = window_start + position as i32;
        let Some(child) = embedded_object_at(connection, &hypertext, char_index).await else {
            continue;
        };
        let Some(flat) = flatten_object_text(connection, &child, 0, &mut budget).await else {
            continue;
        };
        // 只有选区端点上的子对象才可能部分选中；展开过的文本与子对象自身偏移不再对应
        let on_selection_edge = position == sel_start || position + 1 == sel_end;
        let selected = if on_selection_edge && flat.offsets_intact {
            selection_on_object(connection, &child)
                .await
                .filter(|selection| selection.start >= 0)
                .map(|selection| (selection.start as usize, selection.end as usize))
        } else {
            None
        };
        objects[index] = Some(EmbeddedObject {
            text: flat.text,
            selected,
        });
    }
    return objects;
}

/// 对象展开后的文本。
struct FlatText {
    text: String,
    /// text 的字符偏移是否仍与对象自身的 Text 接口一致（没有展开过嵌套占位符）
    offsets_intact: bool,
}

/// 读对象的完整文本，递归展开嵌套的占位符。递归的 async fn 必须手动装箱。
///
/// 没有 Text 接口的对象（图片等）展开为空串；有 Text 接口却读不出来、超出深度或
/// 预算时返回 None，交给调用方按未解析处理。
fn flatten_object_text<'a>(
    connection: &'a AccessibilityConnection,
    object_ref: &'a ObjectRefOwned,
    depth: usize,
    budget: &'a mut usize,
) -> Pin<Box<dyn Future<Output = Option<FlatText>> + Send + 'a>> {
    return Box::pin(async move {
        if depth > MAX_EMBEDDED_DEPTH || *budget == 0 {
            return None;
        }
        *budget -= 1;
        let accessible = accessible_from_ref(connection, object_ref).await.ok()?;
        if !accessible
            .get_interfaces()
            .await
            .ok()?
            .contains(Interface::Text)
        {
            return Some(FlatText {
                text: String::new(),
                offsets_intact: false,
            });
        }
        let text = text_from_ref(connection, object_ref).await.ok()?;
        let char_count = text.character_count().await.ok()?;
        // 子对象大到超过上下文窗口时不再是句内元素，不值得整段读回来
        if char_count > WINDOW_MARGIN * 2 {
            return None;
        }
        let raw = text.get_text(0, char_count).await.ok()?;
        if !raw.contains(EMBEDDED_OBJECT_CHAR) {
            return Some(FlatText {
                text: raw,
                offsets_intact: true,
            });
        }
        let hypertext = hypertext_from_ref(connection, object_ref).await.ok()?;
        let mut flat = String::with_capacity(raw.len());
        for (index, ch) in raw.chars().enumerate() {
            if ch != EMBEDDED_OBJECT_CHAR {
                flat.push(ch);
                continue;
            }
            let child = embedded_object_at(connection, &hypertext, index as i32).await?;
            let child_text = flatten_object_text(connection, &child, depth + 1, budget).await?;
            flat.push_str(&child_text.text);
        }
        return Some(FlatText {
            text: flat,
            offsets_intact: false,
        });
    });
}

/// 上下文窗口的单侧余量（字符数）。
const WINDOW_MARGIN: i32 = 1024;

/// 取句链路：窗口化读选区周边文本（±1024 字符，两端夹到文本边界）→
///（含占位符时展开）→ 字符偏移换算 UTF-16 → 共享 capture 校验锚点并切句。
/// 返回切句结果与真实的所选文本（`word` 中的占位符已展开）；失败返回 Err 由调用方降级。
async fn find_sentence_for_word(
    connection: &AccessibilityConnection,
    selection: &AtspiSelection,
    word: &str,
) -> Result<(SentenceCapture, String), String> {
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

    let sel_start_chars = (selection.start - window_start) as usize;
    if context.contains(EMBEDDED_OBJECT_CHAR) {
        let objects = resolve_embedded_objects(connection, selection, &context, window_start).await;
        let result = capture_with_embedded_objects(
            &context,
            word,
            sel_start_chars,
            &objects,
            window_start as isize,
        )
        .ok_or_else(|| {
            format!(
                "no sentence free of unresolved embedded objects ({} of {} resolved)",
                objects.iter().flatten().count(),
                objects.len()
            )
        })?;
        if result.touched_edge {
            log::info!(
                "sentence touches the edge of a ±{WINDOW_MARGIN}-character context window, \
                 accepting a possibly truncated sentence"
            );
        }
        return Ok((result.capture, result.word));
    }

    // AT-SPI 偏移按字符计，换算为 UTF-16 码元偏移后再切句
    let sel_start_utf16 = char_offset_to_utf16(&context, sel_start_chars).ok_or_else(|| {
        format!(
            "selection offset {sel_start_chars} is out of the context ({} characters)",
            context.chars().count()
        )
    })? as isize;
    // window_loc：窗口起点的文档级偏移（0 表示已对齐文本开头），供触边判定
    match capture_in_context(
        &context,
        word,
        &[sel_start_utf16],
        window_start as isize,
        true,
    ) {
        Some((capture, touched_edge)) => {
            if touched_edge {
                log::info!(
                    "sentence touches the edge of a ±{WINDOW_MARGIN}-character context window \
                     (context {} utf16 units), accepting a possibly truncated sentence",
                    context.encode_utf16().count()
                );
            }
            return Ok((capture, word.to_string()));
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

/// 由对象引用构建 HypertextProxy（对象未实现该接口时，后续方法调用会报错）。
async fn hypertext_from_ref(
    connection: &AccessibilityConnection,
    object_ref: &ObjectRefOwned,
) -> Result<HypertextProxy<'static>, String> {
    let name = object_ref.name().ok_or("the object reference is null")?;
    return HypertextProxy::builder(connection.connection())
        .destination(name.clone())
        .map_err(|error| format!("invalid bus name: {error}"))?
        .path(object_ref.path().clone())
        .map_err(|error| format!("invalid object path: {error}"))?
        .build()
        .await
        .map_err(|error| format!("failed to build a Hypertext proxy: {error}"));
}

/// 由对象引用构建 HyperlinkProxy。
async fn hyperlink_from_ref(
    connection: &AccessibilityConnection,
    object_ref: &ObjectRefOwned,
) -> Result<HyperlinkProxy<'static>, String> {
    let name = object_ref.name().ok_or("the object reference is null")?;
    return HyperlinkProxy::builder(connection.connection())
        .destination(name.clone())
        .map_err(|error| format!("invalid bus name: {error}"))?
        .path(object_ref.path().clone())
        .map_err(|error| format!("invalid object path: {error}"))?
        .build()
        .await
        .map_err(|error| format!("failed to build a Hyperlink proxy: {error}"));
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
        let result = super::atspi_context_with_timeout(false);
        eprintln!(
            "atspi_selected_context() -> {:?}",
            result.map(|context| context.text.chars().count())
        );
    }
}
