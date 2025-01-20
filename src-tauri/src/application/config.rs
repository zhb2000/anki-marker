use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager, State};

use super::logics;
use super::logics::config::{Config, PartialConfig};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Portable(pub bool);

impl Portable {
    pub fn new() -> Result<Self, String> {
        // 如果当前 exe 的旁边存在 config.toml，则认为是便携模式
        let config_path = logics::utils::current_exe_dir()?.join("config.toml");
        let portable = config_path
            .try_exists()
            .map_err(|e| format!("failed to to detect if config.toml exists: {e}"))?;
        return Ok(Portable(portable));
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ConfigPath(pub String);

impl ConfigPath {
    pub fn new(
        portable: bool,
        path_resolver: &tauri::path::PathResolver<impl tauri::Runtime>,
    ) -> Result<Self, String> {
        let config_path = if portable {
            logics::utils::current_exe_dir()?.join("config.toml")
        } else {
            path_resolver
                .app_config_dir()
                .map_err(|e| format!("failed to resolve app config directory: {e}"))?
                .join("config.toml")
        };
        return Ok(ConfigPath(config_path.to_string_lossy().into_owned()));
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn read_config(
    config_path: State<ConfigPath>,
    portable: State<Portable>,
    app: AppHandle,
) -> Result<Config, String> {
    let config_path: &Path = config_path.0.as_ref();
    let portable = portable.0;
    if !config_path
        .try_exists()
        .map_err(|e| format!("failed to to detect if config.toml exists: {e}"))?
    {
        if portable {
            return Err("config.toml does not exist".to_string());
        }
        // 非便携模式下，若 config.toml 不存在，则将模板配置复制到用户配置目录
        let template_path = app
            .path()
            .resolve("resources/config-template.toml", BaseDirectory::Resource)
            .map_err(|e| format!("failed to resolve resources/config-template.toml: {e}"))?;
        logics::config::copy_template_config(template_path, config_path)?;
    }
    return logics::config::read_config(config_path);
}

#[tauri::command(rename_all = "snake_case")]
pub fn commit_config(
    modified: PartialConfig,
    config_path: State<ConfigPath>,
) -> Result<(), String> {
    let config_path: &Path = config_path.0.as_ref();
    return logics::config::commit_config(config_path, modified);
}

#[tauri::command(rename_all = "snake_case")]
pub fn config_path(config_path: State<ConfigPath>) -> String {
    return config_path.0.clone();
}

#[tauri::command(rename_all = "snake_case")]
pub fn is_portable(portable: State<Portable>) -> bool {
    return portable.0;
}

#[tauri::command(rename_all = "snake_case")]
pub fn show_in_explorer(path: String) -> Result<(), String> {
    return logics::utils::show_in_explorer(&path);
}

pub struct IsWatching(pub Mutex<bool>);

impl IsWatching {
    pub fn new() -> Self {
        return IsWatching(Mutex::new(false));
    }
}

/// Return true if the watcher is started successfully, false if it's already started.
#[tauri::command(rename_all = "snake_case")]
pub fn start_config_watcher(
    is_watching: State<IsWatching>,
    config_path: State<ConfigPath>,
    app: AppHandle,
) -> Result<bool, String> {
    let watching = *is_watching
        .0
        .lock()
        .map_err(|e| format!("failed to lock is_watching: {e}"))?;
    if watching {
        return Ok(false);
    }
    let config_path: &Path = config_path.0.as_ref();
    let window = app
        .get_webview_window("main")
        .ok_or("failed to get main window")?;
    let main_window = window.clone();
    let on_change = move || {
        if main_window.emit("config-changed", ()).is_err() {
            println!("failed to emit config-changed event");
        }
    };
    let main_window = window.clone();
    let on_error = move || {
        if main_window.emit("config-watcher-error", ()).is_err() {
            println!("failed to emit config-watcher-error event");
        }
    };
    let timeout = Duration::from_secs(2);
    logics::utils::watch_file_change(config_path, on_change, on_error, timeout)?;
    let mut guard = is_watching
        .0
        .lock()
        .map_err(|e| format!("failed to lock is_watching: {e}"))?;
    *guard = true;
    return Ok(true);
}

#[tauri::command(rename_all = "snake_case")]
pub fn rust_in_release() -> Result<bool, String> {
    return Ok(!cfg!(debug_assertions));
}
