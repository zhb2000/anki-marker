//! 全局快捷键（划词录入句子）：注册、热更新与句子捕获。
//!
//! 目前仅实现 macOS：读取选中文本优先走辅助功能 API（A11y），
//! 目标应用不兼容时自动回退为模拟 Cmd+C 并读取剪贴板，
//! 二者均要求在「系统设置 → 隐私与安全性 → 辅助功能」中授权本应用。

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

use super::config::ConfigPath;
use super::logics;

/// 暂存的句子。
///
/// 主窗口可能已被用户关闭（macOS 点红点仅关窗、进程仍在），此时快捷键触发会重建窗口；
/// 前端重新加载完成之前 emit 的事件会丢失，因此先暂存句子，由前端就绪后通过
/// `take_pending_sentence` 取走。
#[derive(Debug)]
pub struct PendingSentence(pub Mutex<Option<String>>);

impl PendingSentence {
    pub fn new() -> Self {
        return PendingSentence(Mutex::new(None));
    }
}

/// 取走暂存的句子，若无暂存则返回 null。由前端在页面就绪时调用。
#[tauri::command(rename_all = "snake_case")]
pub fn take_pending_sentence(pending: State<PendingSentence>) -> Option<String> {
    return pending.0.lock().ok().and_then(|mut guard| guard.take());
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
#[cfg(target_os = "macos")]
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
#[cfg(not(target_os = "macos"))]
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

/// 申请辅助功能权限：弹出系统授权弹窗；若弹窗曾被拒绝而不再弹出，
/// 则直接打开系统设置的辅助功能面板作为兜底（两种途径用户任选其一完成授权）。
#[tauri::command(rename_all = "snake_case")]
pub fn request_accessibility_trust() {
    #[cfg(target_os = "macos")]
    {
        let _ = macos_accessibility_client::accessibility::application_is_trusted_with_prompt();
        // application_is_trusted_with_prompt 在用户尚未授权时（无论弹窗是否出现）均返回 false，
        // 此时再打开系统设置面板；若面板已打开则只是重新激活，无副作用
        if !macos_accessibility_client::accessibility::application_is_trusted() {
            let _ = std::process::Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                .spawn();
        }
    }
}

/// 全局快捷键被按下时调用：读取当前选中的句子并录入主窗口。
///
/// 时序要求：必须先读取选中文本、再聚焦本应用窗口——若先聚焦，读取（或模拟的
/// Cmd+C）将作用于本应用自身。读取过程可能阻塞数十毫秒（等待剪贴板），故放独立线程执行。
#[cfg(target_os = "macos")]
pub fn on_shortcut_pressed(app: AppHandle) {
    std::thread::spawn(move || {
        let text = match logics::selected_text::get_selected_text() {
            Ok(text) => text.trim().to_string(),
            Err(_) => {
                // 常见原因：未在系统设置中授予本应用辅助功能权限
                if let Err(error) = app.emit("sentence-capture-failed", ()) {
                    log::warn!("failed to emit sentence-capture-failed event: {error}");
                }
                return;
            }
        };
        if text.is_empty() {
            return; // 未选中任何文本，静默忽略
        }
        if let Err(error) = show_and_focus_main_window(&app) {
            log::warn!("failed to show main window: {error}");
        }
        // 暂存句子：若主窗口刚被重建、前端尚未就绪，事件会丢失，前端启动时会取回暂存内容
        if let Ok(mut pending) = app.state::<PendingSentence>().0.lock() {
            *pending = Some(text.clone());
        }
        if let Err(error) = app.emit("sentence-captured", text) {
            log::warn!("failed to emit sentence-captured event: {error}");
        }
    });
}

/// 显示并聚焦主窗口；若主窗口已不存在，则按 tauri.conf.json 中的窗口配置重建。
///
/// 调用时机：划词快捷键触发、点击 Dock 图标。macOS 上点关闭按钮仅隐藏窗口
/// （应用保留在 Dock 栏），正常情况下窗口始终存在，重建仅作兜底。
#[cfg(target_os = "macos")]
pub fn show_and_focus_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        // 切回前台模式——Dock 显示、托盘隐藏
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
    // 切回前台模式——Dock 显示、托盘隐藏
    super::menubar::on_main_window_shown(app);
    return Ok(());
}
