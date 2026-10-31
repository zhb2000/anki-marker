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

/// async + 阻塞线程池：原子写入要 sync_all 落盘，慢磁盘上可能耗时较长，不能占住主线程
/// （快捷键重注册与图标显隐本就会从配置监视线程调用，不要求在主线程执行）
#[tauri::command(rename_all = "snake_case")]
pub async fn commit_config(modified: PartialConfig, app: AppHandle) -> Result<(), String> {
    return logics::utils::run_blocking(move || {
        let config_path = app.state::<ConfigPath>();
        let config_path: &Path = config_path.0.as_ref();
        logics::config::commit_config(config_path, modified)?;
        // 配置保存后立即更新全局快捷键注册，使设置页获得即时反馈；
        // 配置文件监视器随后触发的重注册是幂等的兜底
        super::shortcut::update_from_config(&app);
        // 保存后若应用正处于后台运行，立即按新配置应用 Dock/菜单栏图标显隐
        super::menubar::on_config_changed(&app);
        return Ok(());
    })
    .await;
}

#[tauri::command(rename_all = "snake_case")]
pub fn config_path(config_path: State<ConfigPath>) -> String {
    return config_path.0.clone();
}

#[tauri::command(rename_all = "snake_case")]
pub fn is_portable(portable: State<Portable>) -> bool {
    return portable.0;
}

/// 前端传入的路径必须是配置文件本身。
///
/// 打开/定位文件的命令只为“配置文件”服务（设置页的“打开所在目录”），而系统的打开
/// 操作可以启动任意程序（macOS 的 .app/.command、Windows 的 .exe/.bat），不能接受
/// 任意路径，否则网页里的任何一处注入都能借此执行本地程序。
fn ensure_config_path(path: &str, config_path: &str) -> Result<(), String> {
    if Path::new(path) != Path::new(config_path) {
        return Err(format!(
            "refused to open a path other than the config file: {path}"
        ));
    }
    return Ok(());
}

#[tauri::command(rename_all = "snake_case")]
pub async fn show_in_explorer(path: String, app: AppHandle) -> Result<(), String> {
    ensure_config_path(&path, &app.state::<ConfigPath>().0)?;
    return logics::utils::run_blocking(move || logics::utils::show_in_explorer(&path)).await;
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_filepath(path: String, app: AppHandle) -> Result<(), String> {
    ensure_config_path(&path, &app.state::<ConfigPath>().0)?;
    return logics::utils::run_blocking(move || logics::utils::open_filepath(&path)).await;
}

/// 只允许白名单内的 URL scheme（见 logics::utils::open_in_browser）
#[tauri::command(rename_all = "snake_case")]
pub async fn open_in_browser(url: String) -> Result<(), String> {
    return logics::utils::run_blocking(move || logics::utils::open_in_browser(&url)).await;
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
    let shortcut_app = app.clone();
    let on_change = move || {
        // 配置文件可能被外部编辑（包括 global-shortcut 项），静默重新注册全局快捷键：
        // 不通知前端——设置页保存配置后 watcher 也会触发，通知会导致重复的注册结果提示
        super::shortcut::update_from_config_silently(&shortcut_app);
        // 外部编辑配置文件时，若应用正处于后台运行，按新配置兜底应用 Dock/菜单栏图标显隐
        super::menubar::on_config_changed(&shortcut_app);
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

#[cfg(test)]
mod tests {
    use super::ensure_config_path;

    #[test]
    fn only_config_path_can_be_opened() {
        let config_path = std::env::temp_dir().join("anki-marker").join("config.toml");
        let config_path = config_path.to_string_lossy().into_owned();
        assert!(ensure_config_path(&config_path, &config_path).is_ok());
        for path in [
            "/Applications/Calculator.app",
            "C:\\Windows\\System32\\calc.exe",
            "",
        ] {
            assert!(
                ensure_config_path(path, &config_path).is_err(),
                "path = {path}"
            );
        }
        let sibling = std::env::temp_dir()
            .join("anki-marker")
            .join("evil.command");
        assert!(ensure_config_path(&sibling.to_string_lossy(), &config_path).is_err());
    }
}
