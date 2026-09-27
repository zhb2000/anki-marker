//! 全局快捷键（划词录入句子）：注册、热更新与句子捕获。
//!
//! 三端实现（见 application::logics::selected_text 的各平台模块）：
//! - macOS：辅助功能 API 优先，回退模拟 Cmd+C；需授予辅助功能权限；
//! - Windows：UI Automation（TextPattern）优先，回退模拟 Ctrl+C，无需权限；
//! - Linux：AT-SPI 优先，回退读 PRIMARY 选区，无需权限（仅 X11 会话可用全局快捷键）。
//!
//! 选词取句（word-to-sentence）开启时，划词只需选中一个单词即可自动录入
//! 其所在的整个句子；捕获结果（录入文本 + 取句命中的单词）以
//! sentence-captured 事件发给前端。

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

use super::config::ConfigPath;
use super::logics;

/// 划词捕获结果（sentence-captured 事件载荷与暂存类型）：serde camelCase，前端键 text/word
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedSentence {
    /// 录入文本：取句成功为整句，降级或关闭取句时为所选原文
    text: String,
    /// 取句模式命中的单词（供前端预选），非取句路径为 None
    word: Option<String>,
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

impl CapturedSentence {
    /// 从选词取句结果构造事件载荷；录入文本为空（未选中任何内容）时返回 None。
    /// 单词 trim 后为空则视为无命中（word 置 None）。
    fn from_selected_context(
        context: logics::selected_text::SelectedContext,
    ) -> Option<CapturedSentence> {
        let text = context.text.trim().to_string();
        if text.is_empty() {
            return None;
        }
        let word = context
            .word
            .map(|word| word.trim().to_string())
            .filter(|word| !word.is_empty());
        return Some(CapturedSentence { text, word });
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
/// 显示与聚焦，窗口被重建时前端尚未挂上监听，"开始"事件会丢失。
#[derive(Debug)]
pub struct CaptureSettling(pub Mutex<bool>);

impl CaptureSettling {
    pub fn new() -> Self {
        return CaptureSettling(Mutex::new(false));
    }
}

/// 查询是否正在后台补取完整句子。由前端在页面就绪时调用（与 take_pending_sentence
/// 同属"补齐前端就绪前错过的事件"的手段）。
#[tauri::command(rename_all = "snake_case")]
pub fn is_capture_settling(settling: State<CaptureSettling>) -> bool {
    return settling.0.lock().map(|guard| *guard).unwrap_or(false);
}

/// 更新在途标记并通知前端。emit 可能因前端尚未就绪而丢失，由
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

/// 快捷键注册结果，emit 给前端用于设置页反馈
#[derive(Debug, Clone, serde::Serialize)]
pub struct ShortcutRegistration {
    /// 配置文件中的快捷键字符串，空字符串表示已停用
    shortcut: String,
    success: bool,
    error: Option<String>,
}

/// 最近一次全局快捷键注册结果的缓存。
///
/// 应用启动时的注册发生在前端就绪之前，emit 的事件会丢失，
/// 故缓存注册结果供前端通过 `get_shortcut_registration` 主动查询补齐；
/// 静默注册（配置文件监视器兜底）路径同样更新缓存，保证缓存始终为真实状态。
static LAST_REGISTRATION: Mutex<Option<ShortcutRegistration>> = Mutex::new(None);

/// “划词失败”暂存标记：失败时需弹出主窗口并提示，若前端尚未就绪
/// （窗口刚重建、页面未加载完），emit 的事件会丢失，前端启动时经
/// `take_pending_capture_failure` 取走标记补弹提示。
static PENDING_CAPTURE_FAILURE: Mutex<bool> = Mutex::new(false);

/// 取走“划词失败”暂存标记；无暂存返回 false。由前端在页面就绪时调用。
#[tauri::command(rename_all = "snake_case")]
pub fn take_pending_capture_failure() -> bool {
    return PENDING_CAPTURE_FAILURE
        .lock()
        .map(|mut guard| std::mem::take(&mut *guard))
        .unwrap_or(false);
}

/// 查询最近一次全局快捷键注册结果（含启动时前端尚未就绪而错过 emit 的情况）；无记录返回 null
#[tauri::command(rename_all = "snake_case")]
pub fn get_shortcut_registration() -> Option<ShortcutRegistration> {
    return LAST_REGISTRATION.lock().ok().and_then(|guard| guard.clone());
}

/// 读取配置文件，按其中的 `global-shortcut` 项更新全局快捷键注册，并通知前端注册结果。
///
/// 调用时机：应用启动、设置页保存配置。幂等，可重复调用。
pub fn update_from_config(app: &AppHandle) {
    update_from_config_inner(app, true);
}

/// 读取配置文件并静默更新全局快捷键注册（不通知前端）。
///
/// 供配置文件监视器兜底使用：设置页保存配置后，watcher 也会因文件变化再次触发重注册，
/// 若同样通知前端，用户会看到重复的注册结果提示。
pub fn update_from_config_silently(app: &AppHandle) {
    update_from_config_inner(app, false);
}

fn update_from_config_inner(app: &AppHandle, notify: bool) {
    let config_path = app.state::<ConfigPath>();
    let shortcut = logics::config::read_config(config_path.0.as_str())
        .map(|config| config.global_shortcut().to_string())
        .unwrap_or_default();
    apply_shortcut(app, &shortcut, notify);
}

/// 将全局快捷键注册状态更新为 `shortcut`：空字符串表示注销全部快捷键。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn apply_shortcut(app: &AppHandle, shortcut: &str, notify: bool) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let global_shortcut = app.global_shortcut();
    let _ = global_shortcut.unregister_all();
    let registration = if shortcut.is_empty() {
        ShortcutRegistration {
            shortcut: String::new(),
            success: true,
            error: None,
        }
    } else {
        match global_shortcut.register(shortcut) {
            Ok(()) => ShortcutRegistration {
                shortcut: shortcut.to_string(),
                success: true,
                error: None,
            },
            Err(error) => {
                log::warn!("failed to register global shortcut \"{shortcut}\": {error}");
                ShortcutRegistration {
                    shortcut: shortcut.to_string(),
                    success: false,
                    error: Some(error.to_string()),
                }
            }
        }
    };
    // 更新缓存（无论是否通知前端，缓存始终反映真实注册状态）
    if let Ok(mut guard) = LAST_REGISTRATION.lock() {
        *guard = Some(registration.clone());
    }
    if notify {
        if let Err(error) = app.emit("shortcut-registration", registration) {
            log::warn!("failed to emit shortcut-registration event: {error}");
        }
    }
}

/// 其他平台暂不支持全局快捷键，注册为空操作
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn apply_shortcut(_app: &AppHandle, _shortcut: &str, _notify: bool) {}

/// 查询本应用是否已被授予辅助功能权限（其他平台无此权限概念，视为已授权）。
#[tauri::command(rename_all = "snake_case")]
pub fn is_accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        return macos_accessibility_client::accessibility::application_is_trusted();
    }
    #[cfg(not(target_os = "macos"))]
    {
        return true;
    }
}

/// 申请辅助功能权限：直接打开系统设置的辅助功能面板。
///
/// 不使用 `application_is_trusted_with_prompt`（系统弹窗）：现代 macOS 上该弹窗
/// 没有"允许"按钮、文案不可定制，点击后仍需跳转设置面板手动勾选，只是多一层
/// 中间跳转；而本应用设置页已有自己的引导界面，故直接深链打开设置面板。
/// 用户授权后切回本应用时，由前端窗口焦点监听自动刷新权限状态。
#[tauri::command(rename_all = "snake_case")]
pub fn request_accessibility_trust() {
    #[cfg(target_os = "macos")]
    {
        // 若面板已打开则只是重新激活，无副作用
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }
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

/// 全局快捷键被按下时调用：读取当前选中的句子并录入主窗口。
///
/// 时序要求：必须先读取选中文本、再聚焦本应用窗口——若先聚焦，读取（或模拟的
/// Cmd+C / Ctrl+C）将作用于本应用自身。读取过程可能阻塞（等待剪贴板或跨进程
/// 无障碍调用），故放独立线程执行。
///
/// `trigger` 区分触发来源：菜单触发的路径需要先归还前台（见
/// `restore_foreground_before_capture`）。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub fn on_shortcut_pressed(app: AppHandle, trigger: CaptureTrigger) {
    std::thread::spawn(move || {
        // 菜单触发：取词前先把前台窗口还给用户所在的应用（必须在取词线程里做：
        // 归还带有界等待，不能阻塞主线程的事件循环）
        restore_foreground_before_capture(trigger);
        // 选词取句开关：每次触发时读配置，读取失败默认开启（与配置缺省值一致）
        let word_to_sentence = logics::config::read_config(app.state::<ConfigPath>().0.as_str())
            .map(|config| config.word_to_sentence())
            .unwrap_or(true);
        // 主路径选区文本（重试守卫的比对基准）：取句命中时是词，降级/回退时是所选原文。
        // 后台重试在 get_selected_context 内部就启动，此时主路径结果尚不可知，
        // 故经此共享槽位在捕获返回后回填给重试回调
        let primary_text = std::sync::Arc::new(Mutex::new(None::<String>));
        let context = match logics::selected_text::get_selected_context(
            word_to_sentence,
            retry_callbacks(app.clone(), primary_text.clone()),
        ) {
            Ok(context) => context,
            Err(capture_error) => {
                // 常见原因：macOS 未授予辅助功能权限；Windows 前台窗口以管理员身份
                // 运行（UIPI 拦截）或取词目标不在前台（托盘菜单抢占前台且未归还，
                // 见 selected_text::windows_target）；Linux 目标应用未通过 AT-SPI
                // 暴露选区且 PRIMARY 也为空
                log::warn!("text capture failed: {capture_error}");
                //
                // 失败也要弹出主窗口：否则录入失败对用户完全无感知（分不清是没启动、
                // 卡死还是失败）。先弹窗再 emit，窗口内的前端才能弹出失败提示；
                // 窗口刚重建、前端尚未就绪时经暂存标记兜底补弹
                if let Err(error) = show_and_focus_main_window(&app) {
                    log::warn!("failed to show main window after a capture failure: {error}");
                }
                if let Ok(mut pending) = PENDING_CAPTURE_FAILURE.lock() {
                    *pending = true;
                }
                if let Err(error) = app.emit("sentence-capture-failed", ()) {
                    log::warn!("failed to emit sentence-capture-failed event: {error}");
                }
                return;
            }
        };
        let captured = match CapturedSentence::from_selected_context(context) {
            Some(captured) => captured,
            None => return, // 未选中任何文本，静默忽略
        };
        // 须在弹窗前记录——弹窗期间重试可能已命中
        if let Ok(mut guard) = primary_text.lock() {
            *guard = Some(captured.word.clone().unwrap_or(captured.text.clone()));
        }
        if let Err(error) = show_and_focus_main_window(&app) {
            log::warn!("failed to show main window: {error}");
        }
        // 暂存捕获结果：若主窗口刚被重建、前端尚未就绪，事件会丢失，前端启动时会取回暂存内容
        if let Ok(mut pending) = app.state::<PendingSentence>().0.lock() {
            *pending = Some(captured.clone());
        }
        if let Err(error) = app.emit("sentence-captured", captured) {
            log::warn!("failed to emit sentence-captured event: {error}");
        }
    });
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
        CaptureTrigger::Shortcut => {
            logics::selected_text::windows_target::forget_recorded_target()
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn restore_foreground_before_capture(_trigger: CaptureTrigger) {}

/// 构造后台重试的生命周期回调：驱动"仍在补取"指示状态，并落地补发的完整句子。
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn retry_callbacks(
    app: AppHandle,
    primary_text: std::sync::Arc<Mutex<Option<String>>>,
) -> logics::selected_text::RetryCallbacks {
    let started_app = app.clone();
    return logics::selected_text::RetryCallbacks {
        on_started: Box::new(move || set_capture_settling(&started_app, true)),
        on_finished: Box::new(move |outcome| {
            if let Some(context) = outcome {
                deliver_retry_capture(&app, &primary_text, context);
            }
            // 无论补发成功、被守卫丢弃还是超时放弃，都要结束"仍在补取"状态
            set_capture_settling(&app, false);
        }),
    };
}

/// 后台重试补发结果的落地。
///
/// 重试守卫：重试路径在目标窗口内搜索"持有选区的元素"（Windows）或按 pid 重建
/// 应用元素（macOS），可能撞错控件（残留选区、浏览器 UI 等）——与主路径结果
/// （模拟复制读到的才是用户真实选区）不一致时丢弃，避免用错误内容覆盖用户
/// 已经看到的正确结果。
/// 例外：重试的词漂移但句子包含权威词（Edge PDF 文本层错位，实测选区偏移
/// 2 个字符）时，词替换为权威词后补发——句子是对的，只是词需要修正
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn deliver_retry_capture(
    app: &AppHandle,
    primary_text: &Mutex<Option<String>>,
    context: logics::selected_text::SelectedContext,
) {
    let Some(mut captured) = CapturedSentence::from_selected_context(context) else {
        return;
    };
    if let Ok(Some(primary)) = primary_text.lock().map(|guard| guard.clone()) {
        let word_matches = captured.word.as_deref().is_some_and(|word| word.trim() == primary);
        let text_matches = captured.text.trim() == primary;
        if !word_matches && !text_matches {
            if captured.word.is_some() && captured.text.contains(&primary) {
                log::info!(
                    "retry capture reconciled with the primary result: word {:?} -> {:?}",
                    captured.word,
                    primary
                );
                captured = CapturedSentence { text: captured.text, word: Some(primary) };
            } else {
                log::warn!(
                    "dropping the retry capture: it does not match the primary result \
                     (retry word {:?}, retry text {:.80?}, primary {:?})",
                    captured.word,
                    captured.text,
                    primary
                );
                return;
            }
        }
    }
    if let Ok(mut pending) = app.state::<PendingSentence>().0.lock() {
        *pending = Some(captured.clone());
    }
    if let Err(error) = app.emit("sentence-captured", captured) {
        log::warn!("failed to emit sentence-captured event (retry): {error}");
    }
}

/// 显示并聚焦主窗口；若主窗口已不存在，则按 tauri.conf.json 中的窗口配置重建。
///
/// 调用时机：划词快捷键触发（macOS）、点击 Dock 图标（macOS）、点击托盘图标/托盘
/// 菜单“打开”（Windows/Linux）。后台运行期间点关闭按钮仅隐藏窗口，正常情况下
/// 窗口始终存在，重建仅作兜底。
pub fn show_and_focus_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        // 从最小化状态恢复（show() 对已最小化的窗口不自动还原）
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        // 切回前台模式——macOS 显示 Dock、托盘隐藏；其他平台托盘隐藏
        super::menubar::on_main_window_shown(app);
        return Ok(());
    }
    let window_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
        .ok_or("main window config not found")?;
    let window = tauri::WebviewWindowBuilder::from_config(app, &window_config)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    // tauri.conf.json 中窗口初始 visible: false（防启动闪屏），此处直接显示；
    // 前端就绪后的 show() 是幂等的
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    // 切回前台模式——macOS 显示 Dock、托盘隐藏；其他平台托盘隐藏
    super::menubar::on_main_window_shown(app);
    return Ok(());
}
