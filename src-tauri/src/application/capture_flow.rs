//! 划词捕获的编排：触发 → 取词 → 交付主路径结果 → 落地后台重试的补发。
//!
//! 平台取词本身在 application::logics::selected_text（三端实现见其模块文档）；
//! 这里负责它与界面之间的并发编排：
//! - 取词互斥：同一时间只允许一次取词（模拟复制回退会备份/恢复剪贴板，交错执行会
//!   互相覆盖备份，用户原剪贴板——macOS 还有提示音音量——会被永久替换）；
//! - 取词代次：每次取词递增，后台重试记下自己的代次，代次落后即过期；
//! - 主路径与重试的同步：重试可能先于主路径完成，结果先暂存、待主路径交付后经
//!   守卫补发；主路径失败后重试结果一律丢弃；
//! - 暂存与状态查询：前端尚未就绪（窗口刚重建）时事件会丢失，经 take_* / is_*
//!   命令补齐。
//!
//! 并发状态的判定（RetrySync、reconcile_retry、CaptureGenerations、InFlightGuard）
//! 都是不依赖 AppHandle 的纯逻辑，便于单测；与 Tauri 交互的部分只做转发。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

use super::config::ConfigPath;
use super::logics;
use super::logics::selected_text::SelectedContext;

/// 划词捕获结果（sentence-captured 事件载荷与暂存类型）：serde camelCase，前端键 text/word
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedSentence {
    /// 录入文本：取句成功为整句，降级或关闭取句时为所选原文
    text: String,
    /// 取句模式命中的单词（供前端预选），非取句路径为 None
    word: Option<String>,
    /// 单词在 text 中的 UTF-16 起始偏移：句子内同一单词出现多次时，前端据此预选
    /// 真正被选中的那一处。与 text 切片校验不符、或偏移在链路中被后处理破坏时为 None，
    /// 前端回退到按字符串匹配
    word_offset: Option<usize>,
    /// 是否为“后台补取”的迟到结果：前端据此在用户已改动（编辑句子、输入查询、
    /// 手动标记）时丢弃它，避免覆盖用户的改动
    retry: bool,
}

impl CapturedSentence {
    /// 从选词取句结果构造事件载荷；录入文本为空（未选中任何内容）时返回 None。
    /// 单词 trim 后为空则视为无命中（word 置 None）。
    ///
    /// `retry` 标记迟到的补取结果（见字段说明）。偏移随 text 的 trim 换算，并用
    /// “该位置的切片等于 word”自校验——链路任何一步破坏了偏移，都在这里归零。
    fn from_selected_context(context: SelectedContext, retry: bool) -> Option<CapturedSentence> {
        let text = context.text.trim().to_string();
        if text.is_empty() {
            return None;
        }
        let word = context
            .word
            .map(|word| word.trim().to_string())
            .filter(|word| !word.is_empty());
        // 守门：录入文本必须包含所选词（切句链路已守一次，这里防后处理改写文本——
        // 宁可按降级语义只录词，也不把一句不含所选词的错句交给前端）
        let (text, word) = match word {
            Some(word) if !text.contains(&word) => {
                log::warn!(
                    "captured text does not contain the selected word, degrading to word-only \
                     (word {word:?}, text {:.80?})",
                    text
                );
                (word, None)
            }
            word => (text, word),
        };
        let word_offset = word.as_deref().and_then(|word| {
            let trimmed = context.text.len() - context.text.trim_start().len();
            let offset = context
                .word_offset?
                .checked_sub(context.text[..trimmed].encode_utf16().count())?;
            let matches = text
                .encode_utf16()
                .skip(offset)
                .take(word.encode_utf16().count())
                .eq(word.encode_utf16());
            return matches.then_some(offset);
        });
        return Some(CapturedSentence {
            text,
            word,
            word_offset,
            retry,
        });
    }

    /// 重试守卫的比对基准：取句命中时是词，降级/回退时是所选原文
    fn primary_key(&self) -> String {
        return self.word.clone().unwrap_or(self.text.clone());
    }
}

/// 暂存的划词捕获结果。
///
/// 主窗口可能已被用户关闭（macOS 点红点仅关窗、进程仍在），此时快捷键触发会重建窗口；
/// 前端重新加载完成之前 emit 的事件会丢失，因此先暂存捕获结果，由前端就绪后通过
/// `take_pending_sentence` 取走。
#[derive(Debug)]
pub struct PendingSentence(pub Mutex<Option<CapturedSentence>>);

impl PendingSentence {
    pub fn new() -> Self {
        return PendingSentence(Mutex::new(None));
    }
}

/// 取走暂存的划词捕获结果，若无暂存则返回 null。由前端在页面就绪时调用。
#[tauri::command(rename_all = "snake_case")]
pub fn take_pending_sentence(pending: State<PendingSentence>) -> Option<CapturedSentence> {
    return pending.0.lock().ok().and_then(|mut guard| guard.take());
}

/// “仍在后台补取完整句子”的在途标记（后台重试的存活状态）。
///
/// 供句子面板角上的工作指示器使用：前端据此显示/收束指示环。之所以要能主动查询
/// 而不只靠事件——重试在 `get_selected_context` 内部就已启动，早于本应用窗口的
/// 显示与聚焦，窗口被重建时前端尚未挂上监听，“开始”事件会丢失。
#[derive(Debug)]
pub struct CaptureSettling(pub Mutex<bool>);

impl CaptureSettling {
    pub fn new() -> Self {
        return CaptureSettling(Mutex::new(false));
    }
}

/// 查询是否正在后台补取完整句子。由前端在页面就绪时调用（与 take_pending_sentence
/// 同属“补齐前端就绪前错过的事件”的手段）。
#[tauri::command(rename_all = "snake_case")]
pub fn is_capture_settling(settling: State<CaptureSettling>) -> bool {
    return settling.0.lock().map(|guard| *guard).unwrap_or(false);
}

/// 划词没有录入内容的原因（emit 给前端，用于给出对应的提示文案）。
///
/// 两种原因对用户是不同的事：`Empty` 是“目标应用里没有可取用的选区”（真的没选中，
/// 或应用不向无障碍接口暴露选区），`Failed` 是取词过程报错。以前 `Empty` 是静默的
/// （避免误按快捷键时被打扰），但托盘菜单路径下静默会让用户无法区分“没选中”
/// “应用不支持”与“取词目标取错了”，故两种都反馈。
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureFailure {
    /// 目标应用里没有可取用的选区
    Empty,
    /// 取词过程报错（无障碍查询与模拟复制都没拿到内容）
    Failed,
}

/// “划词失败”暂存标记：失败时需弹出主窗口并提示，若前端尚未就绪
/// （窗口刚重建、页面未加载完），emit 的事件会丢失，前端启动时经
/// `take_pending_capture_failure` 取走标记补弹提示。
static PENDING_CAPTURE_FAILURE: Mutex<Option<CaptureFailure>> = Mutex::new(None);

/// 取走“划词失败”暂存标记（含失败种类）；无暂存返回 null。由前端在页面就绪时调用。
#[tauri::command(rename_all = "snake_case")]
pub fn take_pending_capture_failure() -> Option<CaptureFailure> {
    return PENDING_CAPTURE_FAILURE
        .lock()
        .ok()
        .and_then(|mut guard| guard.take());
}

/// 划词捕获的触发来源。
///
/// 两种来源在“前台窗口归属”上的处境不同：全局快捷键不改变前台窗口，目标应用始终
/// 持有焦点；托盘/Dock 菜单则要先弹菜单，Windows 上菜单弹出前 tray-icon 会把前台
/// 让给自己的隐藏消息窗口，且 Windows 不会自动归还（见
/// logics::selected_text::windows_target 的模块文档）。菜单路径因此需要在取词前
/// 显式归还前台，见 `restore_foreground_before_capture`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTrigger {
    /// 全局快捷键
    Shortcut,
    /// 托盘图标菜单 / macOS Dock 菜单的“划词录入”
    Menu,
}

// #region 并发状态（纯逻辑）

/// 取词在途标记的 RAII 守卫：持有期间其他取词一律被拒，Drop 时复位
/// （含取词线程 panic 的情形）
struct InFlightGuard<'a>(&'a AtomicBool);

impl<'a> InFlightGuard<'a> {
    /// 尝试占用在途标记；已被占用时返回 None
    fn acquire(flag: &'a AtomicBool) -> Option<InFlightGuard<'a>> {
        if flag.swap(true, Ordering::SeqCst) {
            return None;
        }
        return Some(InFlightGuard(flag));
    }
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// 取词代次计数器：每次取词递增，持有旧代次的重试即为过期
struct CaptureGenerations(AtomicU64);

impl CaptureGenerations {
    const fn new() -> Self {
        return CaptureGenerations(AtomicU64::new(0));
    }

    /// 开始新一次取词，返回它的代次（之前所有代次随之过期）
    fn begin(&self) -> u64 {
        return self.0.fetch_add(1, Ordering::SeqCst) + 1;
    }

    fn is_current(&self, generation: u64) -> bool {
        return self.0.load(Ordering::SeqCst) == generation;
    }
}

/// 主路径结果的状态（重试守卫的比对基准）
#[derive(Debug, Default)]
enum PrimaryState {
    /// 主路径尚未返回
    #[default]
    Pending,
    /// 主路径已交付：取句命中时是词，降级/回退时是所选原文
    Ready(String),
    /// 主路径失败或为空（已弹失败提示）：重试结果一律丢弃
    Failed,
}

/// 主路径与后台重试之间的同步槽位（一次取词一个）
#[derive(Debug, Default)]
struct RetrySync {
    primary: PrimaryState,
    /// 主路径尚未返回时就已完成的重试结果，等主路径交付后再经守卫补发
    early: Option<SelectedContext>,
}

impl RetrySync {
    /// 重试取到结果：主路径已交付则返回它（交给守卫补发）；主路径尚未返回则暂存、
    /// 返回 None（由 primary_ready 取出）；主路径已失败则丢弃
    fn on_retry_result(&mut self, context: SelectedContext) -> Option<SelectedContext> {
        match self.primary {
            PrimaryState::Pending => {
                self.early = Some(context);
                return None;
            }
            PrimaryState::Ready(_) => return Some(context),
            PrimaryState::Failed => return None,
        }
    }

    /// 主路径交付：回填比对基准，取出先到的重试结果（若有）
    fn primary_ready(&mut self, primary: String) -> Option<SelectedContext> {
        self.primary = PrimaryState::Ready(primary);
        return self.early.take();
    }

    /// 主路径失败或为空：作废本次的后台重试（含已暂存的结果）
    fn primary_failed(&mut self) {
        self.primary = PrimaryState::Failed;
        self.early = None;
    }

    fn is_failed(&self) -> bool {
        return matches!(self.primary, PrimaryState::Failed);
    }

    /// 已交付的主路径比对基准
    fn primary(&self) -> Option<&str> {
        match &self.primary {
            PrimaryState::Ready(primary) => return Some(primary),
            _ => return None,
        }
    }
}

/// 重试守卫：重试路径在目标窗口内搜索“持有选区的元素”（Windows）或按 pid 重建
/// 应用元素（macOS），可能撞错控件（残留选区、浏览器 UI 等）——与主路径结果
/// （模拟复制读到的才是用户真实选区）不一致时丢弃（返回 None），避免用错误内容
/// 覆盖用户已经看到的正确结果。
/// 例外：重试的词漂移但句子包含权威词（Edge PDF 文本层错位，实测选区偏移
/// 2 个字符）时，词替换为权威词后补发——句子是对的，只是词需要修正
fn reconcile_retry(captured: CapturedSentence, primary: &str) -> Option<CapturedSentence> {
    let word_matches = captured
        .word
        .as_deref()
        .is_some_and(|word| word.trim() == primary);
    let text_matches = captured.text.trim() == primary;
    if word_matches || text_matches {
        return Some(captured);
    }
    if captured.word.is_some() && captured.text.contains(primary) {
        log::info!(
            "retry capture reconciled with the primary result: word {:?} -> {:?}",
            captured.word,
            primary
        );
        return Some(CapturedSentence {
            text: captured.text,
            word: Some(primary.to_string()),
            word_offset: None,
            retry: true,
        });
    }
    log::warn!(
        "dropping the retry capture: it does not match the primary result \
         (retry word {:?}, retry text {:.80?}, primary {:?})",
        captured.word,
        captured.text,
        primary
    );
    return None;
}

// #endregion

/// 取词在途标记：同一时间只允许一次取词（见 start_capture）。
static CAPTURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// 取词代次：每次取词递增。后台重试记下自己的代次，代次落后即视为过期
/// （用户已发起新一次取词），停止轮询、不再补发
static CAPTURE_GENERATIONS: CaptureGenerations = CaptureGenerations::new();

/// 全局快捷键被按下、或托盘/Dock 菜单点了“划词录入”时调用：读取当前选中的句子
/// 并录入主窗口。
///
/// 时序要求：必须先读取选中文本、再聚焦本应用窗口——若先聚焦，读取（或模拟的
/// Cmd+C / Ctrl+C）将作用于本应用自身。读取过程可能阻塞（等待剪贴板或跨进程
/// 无障碍调用），故放独立线程执行。
///
/// `trigger` 区分触发来源：菜单触发的路径需要先归还前台（见
/// `restore_foreground_before_capture`）。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub fn start_capture(app: AppHandle, trigger: CaptureTrigger) {
    // 取词互斥：上一次取词仍在途时丢弃新的触发。两次取词交错会互相覆盖剪贴板备份
    // （模拟复制回退路径先备份、再复制、后恢复），用户原剪贴板（macOS 还有提示音音量）
    // 会被永久替换；两次结果先后 emit 也会让界面被覆盖两次
    let Some(in_flight) = InFlightGuard::acquire(&CAPTURE_IN_FLIGHT) else {
        log::info!("a capture is already in progress, ignoring the new trigger");
        return;
    };
    std::thread::spawn(move || {
        let _in_flight = in_flight;
        // 菜单触发：取词前先把前台窗口还给用户所在的应用（必须在取词线程里做：
        // 归还带有界等待，不能阻塞主线程的事件循环）
        restore_foreground_before_capture(trigger);
        // 选词取句开关：每次触发时读配置，读取失败默认开启（与配置缺省值一致）
        let word_to_sentence = logics::config::read_config(app.state::<ConfigPath>().0.as_str())
            .map(|config| config.word_to_sentence())
            .unwrap_or(true);
        let generation = CAPTURE_GENERATIONS.begin();
        // 上一次取词的重试随代次递增而过期，过期回调不再改动“仍在补取”状态——
        // 这里统一收掉旧指示；本次若启动新重试，其 on_started 会再置 true
        if app
            .state::<CaptureSettling>()
            .0
            .lock()
            .map(|guard| *guard)
            .unwrap_or(false)
        {
            set_capture_settling(&app, false);
        }
        // 主路径与重试的同步槽位：后台重试在 get_selected_context 内部就启动，此时
        // 主路径结果尚不可知，故经此槽位在捕获返回后回填重试守卫的比对基准
        let retry_sync = std::sync::Arc::new(Mutex::new(RetrySync::default()));
        let context = match logics::selected_text::get_selected_context(
            word_to_sentence,
            retry_callbacks(app.clone(), generation, retry_sync.clone()),
        ) {
            Ok(context) => context,
            Err(capture_error) => {
                // 常见原因：macOS 未授予辅助功能权限；Windows 前台窗口以管理员身份
                // 运行（UIPI 拦截）或取词目标不在前台（托盘菜单抢占前台且未归还，
                // 见 selected_text::windows_target）；Linux 目标应用未通过 AT-SPI
                // 暴露选区且 PRIMARY 也为空
                log::warn!("text capture failed: {capture_error}");
                // 主路径已失败（界面会弹失败提示）：作废本次的后台重试，免得提示之后
                // 句子又被悄悄填进来
                mark_primary_failed(&retry_sync);
                report_capture_failure(&app, CaptureFailure::Failed);
                return;
            }
        };
        let captured = match CapturedSentence::from_selected_context(context, false) {
            Some(captured) => captured,
            None => {
                // 目标应用里没有可取用的选区：真的没选中，或应用不向无障碍接口暴露选区。
                // 与取词报错同样给反馈——静默时用户分不清“没选中”“应用不支持”与
                // “取词目标取错了”（托盘菜单路径下尤其明显）
                log::info!("no text captured: the target app returned an empty selection");
                mark_primary_failed(&retry_sync);
                report_capture_failure(&app, CaptureFailure::Empty);
                return;
            }
        };
        // 回填重试守卫的比对基准，并取出“先于主路径完成”的重试结果（若有）：
        // 主路径（模拟复制、PDF 精修）可能比重试的首轮轮询还慢，此前到达的重试结果
        // 暂存在槽位里，须在主路径结果之后经守卫补发，否则会被主路径的降级结果覆盖
        let early_retry = retry_sync
            .lock()
            .ok()
            .and_then(|mut sync| sync.primary_ready(captured.primary_key()));
        if let Err(error) = super::window::show_and_focus_main_window(&app) {
            log::warn!("failed to show main window: {error}");
        }
        emit_captured(&app, captured);
        if let Some(context) = early_retry {
            if CAPTURE_GENERATIONS.is_current(generation) {
                deliver_retry_capture(&app, &retry_sync, context);
            }
        }
    });
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn mark_primary_failed(retry_sync: &Mutex<RetrySync>) {
    if let Ok(mut sync) = retry_sync.lock() {
        sync.primary_failed();
    }
}

/// 发出 sentence-captured 事件，并暂存结果：若主窗口刚被重建、前端尚未就绪，事件会丢失，
/// 前端启动时取回暂存内容（前端收到事件时也会取走暂存，避免日后重载时回放旧句子）
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn emit_captured(app: &AppHandle, captured: CapturedSentence) {
    if let Ok(mut pending) = app.state::<PendingSentence>().0.lock() {
        *pending = Some(captured.clone());
    }
    if let Err(error) = app.emit("sentence-captured", captured) {
        log::warn!("failed to emit sentence-captured event: {error}");
    }
}

/// 更新“仍在补取”标记并通知前端。emit 可能因前端尚未就绪而丢失，由
/// `is_capture_settling` 的主动查询兜底。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn set_capture_settling(app: &AppHandle, settling: bool) {
    match app.state::<CaptureSettling>().0.lock() {
        Ok(mut guard) => *guard = settling,
        Err(error) => log::warn!("failed to update the settling flag: {error}"),
    }
    if let Err(error) = app.emit("sentence-capture-settling", settling) {
        log::warn!("failed to emit sentence-capture-settling event: {error}");
    }
}

/// 划词没有录入内容时的统一反馈：弹出主窗口，并让前端提示原因（`failure` 区分
/// “没有取到选区”与“取词报错”）。
///
/// 弹出主窗口是必需的：否则失败对用户完全无感知（分不清是没启动、卡死还是失败）。
/// 先弹窗再 emit，窗口内的前端才能弹出提示；窗口刚重建、前端尚未就绪时经暂存标记
/// 兜底补弹。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn report_capture_failure(app: &AppHandle, failure: CaptureFailure) {
    if let Err(error) = super::window::show_and_focus_main_window(app) {
        log::warn!("failed to show main window after a capture failure: {error}");
    }
    if let Ok(mut pending) = PENDING_CAPTURE_FAILURE.lock() {
        *pending = Some(failure);
    }
    if let Err(error) = app.emit("sentence-capture-failed", failure) {
        log::warn!("failed to emit sentence-capture-failed event: {error}");
    }
}

/// 菜单触发的取词在捕获前归还前台窗口（仅 Windows 需要）。
///
/// Windows 的托盘菜单弹出前会抢走前台窗口，且系统不会自动归还（实机验证：
/// 菜单关闭后 800ms 前台仍是托盘窗口），取词因此会落到我们自己的窗口上——
/// UIA 焦点元素没有 TextPattern、注入的 Ctrl+C 也送不出去。
/// macOS 的菜单栏菜单不激活本应用，无此问题；**Linux 有同类问题但尚未处理**
/// （托盘菜单由宿主面板持有，活动窗口不在目标应用上）——见 selected_text::linux
/// 模块文档里的待办。快捷键路径无需归还，故空实现。
#[cfg(target_os = "windows")]
fn restore_foreground_before_capture(trigger: CaptureTrigger) {
    match trigger {
        CaptureTrigger::Menu => logics::selected_text::windows_target::ensure_foreign_foreground(),
        // 快捷键路径：前台窗口没被动过，无需归还；顺手清掉上一条托盘记录，
        // 免得陈旧记录被它的重试目标解析捡到
        CaptureTrigger::Shortcut => logics::selected_text::windows_target::forget_recorded_target(),
    }
}

#[cfg(not(target_os = "windows"))]
fn restore_foreground_before_capture(_trigger: CaptureTrigger) {}

/// 构造后台重试的生命周期回调：驱动“仍在补取”指示状态，并落地补发的完整句子。
///
/// 回调都带代次检查：过期的重试（用户已发起新一次取词）既不补发结果，也不改动
/// “仍在补取”状态——否则旧重试结束时会把新重试的指示环提前收掉
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn retry_callbacks(
    app: AppHandle,
    generation: u64,
    retry_sync: std::sync::Arc<Mutex<RetrySync>>,
) -> logics::selected_text::RetryCallbacks {
    let started_app = app.clone();
    let cancel_sync = retry_sync.clone();
    return logics::selected_text::RetryCallbacks {
        on_started: Box::new(move || {
            if CAPTURE_GENERATIONS.is_current(generation) {
                set_capture_settling(&started_app, true);
            }
        }),
        on_finished: Box::new(move |outcome| {
            if !CAPTURE_GENERATIONS.is_current(generation) {
                return;
            }
            if let Some(context) = outcome {
                let deliver_now = retry_sync
                    .lock()
                    .ok()
                    .and_then(|mut sync| sync.on_retry_result(context));
                if let Some(context) = deliver_now {
                    deliver_retry_capture(&app, &retry_sync, context);
                }
            }
            // 无论补发成功、被守卫丢弃、暂存待补发还是超时放弃，都要结束“仍在补取”状态
            set_capture_settling(&app, false);
        }),
        is_cancelled: Box::new(move || {
            !CAPTURE_GENERATIONS.is_current(generation)
                || cancel_sync
                    .lock()
                    .map(|sync| sync.is_failed())
                    .unwrap_or(false)
        }),
    };
}

/// 后台重试补发结果的落地：经重试守卫（reconcile_retry）比对主路径结果后补发。
/// 只在主路径已交付时调用；主路径基准缺失时不补发。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn deliver_retry_capture(app: &AppHandle, retry_sync: &Mutex<RetrySync>, context: SelectedContext) {
    let Some(captured) = CapturedSentence::from_selected_context(context, true) else {
        return;
    };
    let Some(primary) = retry_sync
        .lock()
        .ok()
        .and_then(|sync| sync.primary().map(str::to_string))
    else {
        return;
    };
    if let Some(captured) = reconcile_retry(captured, &primary) {
        emit_captured(app, captured);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::{reconcile_retry, CaptureGenerations, CapturedSentence, InFlightGuard, RetrySync};
    use crate::application::logics::selected_text::SelectedContext;

    fn context(text: &str, word: Option<&str>) -> SelectedContext {
        return SelectedContext {
            text: text.to_string(),
            word: word.map(str::to_string),
            word_offset: None,
        };
    }

    fn retry_capture(text: &str, word: Option<&str>) -> CapturedSentence {
        return CapturedSentence::from_selected_context(context(text, word), true)
            .expect("payload");
    }

    /// 文本完整保留、偏移随 trim 换算。
    /// 回归：文本曾被截成“所选词之前的句子前缀”（选中 design 只录入 We）
    #[test]
    fn payload_keeps_the_whole_text() {
        let captured = CapturedSentence::from_selected_context(
            SelectedContext {
                text: "  We design a Transformer-based encoder-decoder model.".to_string(),
                word: Some("design".to_string()),
                word_offset: Some(5),
            },
            false,
        )
        .expect("payload");
        assert_eq!(
            captured.text,
            "We design a Transformer-based encoder-decoder model."
        );
        assert_eq!(captured.word.as_deref(), Some("design"));
        assert_eq!(captured.word_offset, Some(3)); // 减去两个前导空格
        assert!(!captured.retry);
    }

    /// 偏移与文本不符（链路中某一步改写过文本）：只作废偏移，文本与词照常交付
    #[test]
    fn payload_drops_unverified_offset() {
        let captured = CapturedSentence::from_selected_context(
            SelectedContext {
                text: "We design models.".to_string(),
                word: Some("design".to_string()),
                word_offset: Some(7),
            },
            true,
        )
        .expect("payload");
        assert_eq!(captured.word_offset, None);
        assert!(captured.retry);
    }

    /// 守门：文本不含所选词时降级为仅录词，不把错句交给前端
    #[test]
    fn payload_degrades_when_text_lacks_the_word() {
        let captured = CapturedSentence::from_selected_context(
            SelectedContext {
                text: "We".to_string(),
                word: Some("design".to_string()),
                word_offset: Some(3),
            },
            false,
        )
        .expect("payload");
        assert_eq!(captured.text, "design");
        assert_eq!(captured.word, None);
        assert_eq!(captured.word_offset, None);
    }

    /// 空选区不产生载荷
    #[test]
    fn payload_rejects_blank_text() {
        assert!(CapturedSentence::from_selected_context(context("  \n ", None), false).is_none());
    }

    /// 比对基准：取句命中时是词，否则是原文
    #[test]
    fn primary_key_prefers_the_word() {
        let with_word = retry_capture("We design models.", Some("design"));
        assert_eq!(with_word.primary_key(), "design");
        let text_only = retry_capture("design", None);
        assert_eq!(text_only.primary_key(), "design");
    }

    /// 重试先于主路径完成：暂存，主路径交付时取出
    #[test]
    fn early_retry_is_stashed_until_the_primary_is_ready() {
        let mut sync = RetrySync::default();
        assert!(sync
            .on_retry_result(context("We design models.", Some("design")))
            .is_none());
        assert_eq!(sync.primary(), None);
        let early = sync
            .primary_ready("design".to_string())
            .expect("stashed retry");
        assert_eq!(early.text, "We design models.");
        assert_eq!(sync.primary(), Some("design"));
        // 已取出，不会重复补发
        assert!(sync.primary_ready("design".to_string()).is_none());
    }

    /// 主路径已交付：重试结果立即交给守卫
    #[test]
    fn retry_after_the_primary_is_delivered_now() {
        let mut sync = RetrySync::default();
        assert!(sync.primary_ready("design".to_string()).is_none());
        let now = sync
            .on_retry_result(context("We design models.", Some("design")))
            .expect("deliver now");
        assert_eq!(now.word.as_deref(), Some("design"));
    }

    /// 主路径失败：已暂存与之后到达的重试结果都丢弃，并对外报告过期
    #[test]
    fn primary_failure_drops_retries() {
        let mut sync = RetrySync::default();
        assert!(!sync.is_failed());
        assert!(sync
            .on_retry_result(context("We design models.", Some("design")))
            .is_none());
        sync.primary_failed();
        assert!(sync.is_failed());
        assert_eq!(sync.primary(), None);
        assert!(sync
            .on_retry_result(context("We design models.", Some("design")))
            .is_none());
    }

    /// 守卫：词或原文与主路径一致时原样补发
    #[test]
    fn reconcile_keeps_matching_retries() {
        let by_word = retry_capture("We design models.", Some("design"));
        assert_eq!(reconcile_retry(by_word.clone(), "design"), Some(by_word));
        let by_text = retry_capture("design", None);
        assert_eq!(reconcile_retry(by_text.clone(), "design"), Some(by_text));
    }

    /// 守卫：词漂移但句子含权威词时，词替换为权威词、偏移作废，句子完整保留
    #[test]
    fn reconcile_replaces_a_drifted_word() {
        let drifted = retry_capture("We design models.", Some("esign m"));
        let reconciled = reconcile_retry(drifted, "design").expect("reconciled");
        assert_eq!(reconciled.text, "We design models.");
        assert_eq!(reconciled.word.as_deref(), Some("design"));
        assert_eq!(reconciled.word_offset, None);
        assert!(reconciled.retry);
    }

    /// 守卫：与主路径不一致（撞错控件）时丢弃
    #[test]
    fn reconcile_drops_mismatching_retries() {
        let wrong = retry_capture("Some other sentence.", Some("other"));
        assert_eq!(reconcile_retry(wrong, "design"), None);
        // 原文路径（无词）即使句子包含主路径词也不替换——没有可修正的词
        let text_only = retry_capture("We design models.", None);
        assert_eq!(reconcile_retry(text_only, "design"), None);
    }

    /// 代次：开始新一次取词后旧代次过期
    #[test]
    fn generations_expire_older_captures() {
        let generations = CaptureGenerations::new();
        let first = generations.begin();
        assert!(generations.is_current(first));
        let second = generations.begin();
        assert!(!generations.is_current(first));
        assert!(generations.is_current(second));
    }

    /// 在途互斥：占用期间拒绝新的取词，释放（含 panic 展开）后可再次占用
    #[test]
    fn in_flight_guard_rejects_concurrent_captures() {
        let flag = AtomicBool::new(false);
        let guard = InFlightGuard::acquire(&flag).expect("first capture");
        assert!(InFlightGuard::acquire(&flag).is_none());
        drop(guard);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = InFlightGuard::acquire(&flag).expect("after release");
            panic!("capture thread panicked");
        }));
        assert!(result.is_err());
        assert!(InFlightGuard::acquire(&flag).is_some());
    }
}
