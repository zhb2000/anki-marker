//! 全局快捷键（划词录入句子）：注册、热更新、录制期间的暂停与辅助功能权限。
//!
//! 快捷键按下后的取词编排（互斥、代次、后台重试补发、暂存）见 capture_flow 模块；
//! 三端取词实现见 application::logics::selected_text 的各平台模块：
//! - macOS：辅助功能 API 优先，回退模拟 Cmd+C；需授予辅助功能权限；
//! - Windows：UI Automation（TextPattern）优先，回退模拟 Ctrl+C，无需权限；
//! - Linux：AT-SPI 优先，回退读 PRIMARY 选区，无需权限（仅 X11 会话可用全局快捷键）。

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use super::config::ConfigPath;
use super::logics;

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
    return LAST_REGISTRATION
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
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
    match logics::config::read_config(config_path.0.as_str()) {
        Ok(config) => apply_shortcut(app, config.global_shortcut(), notify),
        Err(error) => {
            // 读取失败（语法错误、文件被占用或正写到一半）：保持现有注册不动，
            // 不能当作“未设置快捷键”把它注销掉；上报失败让设置页看得到原因
            log::warn!(
                "failed to read the config file, keeping the current global shortcut: {error}"
            );
            let shortcut = LAST_REGISTRATION
                .lock()
                .ok()
                .and_then(|guard| {
                    guard
                        .as_ref()
                        .map(|registration| registration.shortcut.clone())
                })
                .unwrap_or_default();
            record_registration(
                app,
                ShortcutRegistration {
                    shortcut,
                    success: false,
                    error: Some(format!("读取配置文件失败：{error}")),
                },
                notify,
            );
        }
    }
}

/// 录制快捷键期间暂停全局快捷键：否则按下当前已注册的组合会被系统级热键吃掉，
/// 录制控件收不到按键，还会意外触发一次取词。录制结束后调用 resume_global_shortcut 恢复。
#[tauri::command(rename_all = "snake_case")]
pub fn suspend_global_shortcut(app: AppHandle) {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        if let Err(error) = app.global_shortcut().unregister_all() {
            log::warn!("failed to suspend the global shortcut: {error}");
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = app;
}

/// 结束录制后按配置恢复全局快捷键注册（不通知前端：录制结果若有变化，保存配置时会通知）。
#[tauri::command(rename_all = "snake_case")]
pub fn resume_global_shortcut(app: AppHandle) {
    update_from_config_silently(&app);
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
    record_registration(app, registration, notify);
}

/// 记录注册结果：更新缓存（无论是否通知前端，缓存始终反映真实注册状态），按需通知前端
fn record_registration(app: &AppHandle, registration: ShortcutRegistration, notify: bool) {
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
/// 没有“允许”按钮、文案不可定制，点击后仍需跳转设置面板手动勾选，只是多一层
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
