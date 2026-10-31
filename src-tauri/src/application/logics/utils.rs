use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::JoinHandle;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_full::new_debouncer;

/// 在异步运行时的阻塞线程池中执行同步操作（起子进程、文件 IO、SQLite 查询等）。
///
/// Tauri 的同步命令在 IPC 回调所在的主线程内联执行，耗时操作会冻结界面与托盘；
/// 命令改为 async 后经此转到阻塞线程池，避免占住主线程或异步工作线程。
pub async fn run_blocking<T: Send + 'static>(
    func: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    return tauri::async_runtime::spawn_blocking(func)
        .await
        .map_err(|e| format!("blocking task failed: {e}"))?;
}

/// 启动子进程，并由后台线程等待其退出、回收进程表项。
///
/// 直接丢弃 `Child` 不会 wait：Unix 上子进程退出后会以僵尸进程残留到本应用退出，
/// Linux 上被拉起的 Anki 退出后仍会被按进程名检测为“正在运行”。
pub fn spawn_and_reap(command: &mut Command) -> std::io::Result<()> {
    let mut child = command.spawn()?;
    let spawned = std::thread::Builder::new()
        .name("child-reaper".to_string())
        .spawn(move || {
            if let Err(e) = child.wait() {
                log::warn!("failed to wait for child process: {e}");
            }
        });
    if let Err(e) = spawned {
        log::warn!("failed to spawn the child reaper thread: {e}");
    }
    return Ok(());
}

/// 以系统默认方式打开文件或 URL（Windows：ShellExecuteW）。
///
/// 取代 `cmd /C start`：cmd 会把参数中的 `&`、`^`、`%VAR%` 当作元字符解释，
/// 路径或 URL 含这些字符时会被截断甚至注入命令（Rust 的参数转义不保护显式的 cmd /C）。
/// `show_cmd` 为窗口显示方式（如 SW_SHOWNORMAL、SW_SHOWMINNOACTIVE）。
#[cfg(target_os = "windows")]
pub fn shell_execute(
    file: &str,
    show_cmd: windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD,
) -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::ShellExecuteW;

    let file = HSTRING::from(file);
    // ShellExecute 可能委托给 Shell 扩展执行，MSDN 要求调用线程先初始化 COM（STA）；
    // 放到独立的短命线程里做，避免改变调用方线程（异步运行时线程池）的 COM 套间
    let worker = std::thread::spawn(move || {
        let code = unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            // 动词传空：使用文件类型的默认动词，与 start 的行为一致
            let instance = ShellExecuteW(
                None,
                PCWSTR::null(),
                PCWSTR(file.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                show_cmd,
            );
            if init.is_ok() {
                CoUninitialize();
            }
            instance.0 as isize
        };
        return code;
    });
    let code = worker
        .join()
        .map_err(|_| "ShellExecuteW worker thread panicked".to_string())?;
    // 返回值大于 32 表示成功，否则为错误码（如 2 = 找不到文件、31 = 无关联程序）
    if code <= 32 {
        return Err(format!("ShellExecuteW failed with code {code}"));
    }
    return Ok(());
}

pub fn show_in_explorer(path: impl AsRef<Path>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    fn inner(path: &str) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        // Windows 路径本就不允许含双引号；防御性拒绝，保证下面手工加的引号不会被提前闭合
        if path.contains('"') {
            return Err(format!("invalid path: {path}"));
        }
        // explorer 自行解析命令行（不按 CRT 规则），`/select,` 后的路径要整体加引号才能
        // 容纳空格与逗号；用 raw_arg 原样传递，避免 Rust 按 CRT 规则再转义一遍引号。
        // 不经 cmd 转发，路径中的 & ^ % 等不会被当作元字符
        let mut command = Command::new("explorer");
        command.raw_arg(format!("/select,\"{path}\""));
        spawn_and_reap(&mut command).map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    fn inner(path: &str) -> Result<(), String> {
        spawn_and_reap(Command::new("open").args(["-R", path])).map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    fn inner(path: &str) -> Result<(), String> {
        let dir_path = std::path::Path::new(path)
            .parent()
            .ok_or("Failed to get parent directory")?
            .to_str()
            .ok_or("Parent directory is not valid utf-8")?;
        spawn_and_reap(Command::new("xdg-open").arg(dir_path)).map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    fn inner(_: &str) -> Result<(), String> {
        Err(format!(
            "show_in_explorer is not implemented on this platform: {}",
            std::env::consts::OS
        ))
    }

    let path = path
        .as_ref()
        .to_str()
        .ok_or("path is not valid utf-8")?
        .replace(['/', '\\'], std::path::MAIN_SEPARATOR_STR);
    return inner(&path);
}

pub fn open_filepath(path: impl AsRef<Path>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    fn inner(path: &str) -> Result<(), String> {
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        return shell_execute(path, SW_SHOWNORMAL);
    }

    #[cfg(target_os = "macos")]
    fn inner(path: &str) -> Result<(), String> {
        spawn_and_reap(Command::new("open").arg(path)).map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    fn inner(path: &str) -> Result<(), String> {
        spawn_and_reap(Command::new("xdg-open").arg(path)).map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    fn inner(_: &str) -> Result<(), String> {
        Err(format!(
            "open_filepath is not implemented on this platform: {}",
            std::env::consts::OS
        ))
    }

    let path = path
        .as_ref()
        .to_str()
        .ok_or("path is not valid utf-8")?
        .replace(['/', '\\'], std::path::MAIN_SEPARATOR_STR);
    return inner(&path);
}

/// open_in_browser 允许打开的 URL scheme（小写）。
///
/// 前端只需要：网页（http/https）、邮件（mailto）、系统设置面板（macOS 的
/// x-apple.systempreferences、Windows 的 ms-settings，见设置页“登录项”入口）。
/// 其余 scheme（file:、自定义协议等）可经系统打开任意本地程序，一律拒绝。
const ALLOWED_URL_SCHEMES: [&str; 5] = [
    "http",
    "https",
    "mailto",
    "x-apple.systempreferences",
    "ms-settings",
];

/// 校验 URL 是否允许交给系统打开：scheme 必须在白名单内（大小写不敏感），
/// http/https 必须带 `//` 主机部分，且不得含控制字符。
fn check_url_allowed(url: &str) -> Result<(), String> {
    let Some((scheme, rest)) = url.split_once(':') else {
        return Err(format!("refused to open URL without a scheme: {url}"));
    };
    let scheme = scheme.to_ascii_lowercase();
    if !ALLOWED_URL_SCHEMES.contains(&scheme.as_str()) {
        return Err(format!("refused to open URL with scheme `{scheme}`: {url}"));
    }
    if (scheme == "http" || scheme == "https") && !rest.starts_with("//") {
        return Err(format!("refused to open malformed URL: {url}"));
    }
    if url.chars().any(char::is_control) {
        return Err(format!(
            "refused to open URL containing control characters: {url:?}"
        ));
    }
    return Ok(());
}

pub fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    fn inner(url: &str) -> Result<(), String> {
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        return shell_execute(url, SW_SHOWNORMAL);
    }

    #[cfg(target_os = "macos")]
    fn inner(url: &str) -> Result<(), String> {
        spawn_and_reap(Command::new("open").arg(url)).map_err(|e| e.to_string())?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn inner(url: &str) -> Result<(), String> {
        spawn_and_reap(Command::new("xdg-open").arg(url)).map_err(|e| e.to_string())?;
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    fn inner(_: &str) -> Result<(), String> {
        Err(format!(
            "open_in_browser is not implemented on this platform: {}",
            std::env::consts::OS
        ))
    }

    check_url_allowed(url)?;
    inner(url)
}

pub fn current_exe_dir() -> Result<PathBuf, String> {
    let exe_path =
        std::env::current_exe().map_err(|e| format!("failed to get current exe path: {}", e))?;
    let exe_dir = exe_path
        .parent()
        .ok_or("failed to get current exe directory")?;
    return Ok(exe_dir.to_path_buf());
}

/// 路径是符号链接（如用 dotfiles 管理配置文件）时返回其指向的真实路径，否则原样返回。
///
/// 原子写入要在真实文件所在目录 rename，否则会用普通文件替换掉链接本身；
/// 监视也要盯真实文件所在的目录，才能收到经链接目标发生的修改。
pub fn resolve_symlink(path: &Path) -> PathBuf {
    let is_symlink = std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    if is_symlink {
        if let Ok(real_path) = std::fs::canonicalize(path) {
            return real_path;
        }
    }
    return path.to_path_buf();
}

/// 原子地写入文件：先写同目录下的临时文件并 sync_all 落盘，再 rename 覆盖目标。
///
/// `std::fs::write` 先截断再写入，中途崩溃或断电会留下空文件或残缺文件；rename 在同一
/// 文件系统内是原子的，读者要么看到旧内容、要么看到完整的新内容。Windows 上
/// `std::fs::rename` 以“替换已存在文件”的语义实现（MoveFileExW + MOVEFILE_REPLACE_EXISTING
/// 或 POSIX 语义的 FileRenameInfoEx），可直接覆盖目标。
pub fn write_file_atomically(path: impl AsRef<Path>, contents: &[u8]) -> Result<(), String> {
    fn inner(path: &Path, contents: &[u8]) -> Result<(), String> {
        use std::io::Write;

        let path = resolve_symlink(path);
        let dir = path
            .parent()
            .ok_or_else(|| format!("failed to get parent directory of {}", path.display()))?;
        let file_name = path
            .file_name()
            .ok_or_else(|| format!("invalid file path: {}", path.display()))?;
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(file_name);
        temp_name.push(".tmp");
        let temp_path = dir.join(temp_name);

        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::File::create(&temp_path)?;
            // 沿用原文件的权限（配置中含 API Key，用户可能已收紧权限）
            #[cfg(unix)]
            if let Ok(metadata) = std::fs::metadata(&path) {
                file.set_permissions(metadata.permissions())?;
            }
            file.write_all(contents)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temp_path, &path)?;
            // 目录项的变更也落盘，确保断电后 rename 不会丢失（尽力而为）
            #[cfg(unix)]
            if let Ok(dir_file) = std::fs::File::open(dir) {
                let _ = dir_file.sync_all();
            }
            return Ok(());
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!("failed to write {}: {e}", path.display()));
        }
        return Ok(());
    }
    return inner(path.as_ref(), contents);
}

/// 文件监视事件是否涉及目标文件：路径的文件名与目标一致，或事件要求重新扫描
/// （事件队列溢出等，此时无法确定哪些文件变了，按“可能变了”处理）。
fn is_relevant_event(event: &notify::Event, file_name: &OsStr) -> bool {
    return event.need_rescan()
        || event
            .paths
            .iter()
            .any(|path| path.file_name() == Some(file_name));
}

/// 监视文件变化。
///
/// 监视的是文件所在的**目录**，再按文件名过滤事件：inotify 等机制把监视绑定在 inode 上，
/// 编辑器“写临时文件再 rename”的保存方式（以及本应用的原子写入）会换掉 inode，
/// 直接监视文件会在第一次这样的保存后永久失效。
pub fn watch_file_change(
    file_path: impl AsRef<Path>,
    on_change: impl Fn() + Send + 'static,
    on_error: impl Fn() + Send + 'static,
    timeout: Duration,
) -> Result<JoinHandle<()>, String> {
    fn inner(
        file_path: &Path,
        on_change: impl Fn() + Send + 'static,
        on_error: impl Fn() + Send + 'static,
        timeout: Duration,
    ) -> Result<JoinHandle<()>, String> {
        let file_path = resolve_symlink(file_path);
        let dir_path = file_path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .ok_or_else(|| format!("failed to get parent directory of {}", file_path.display()))?
            .to_path_buf();
        let file_name = file_path
            .file_name()
            .ok_or_else(|| format!("invalid file path: {}", file_path.display()))?
            .to_os_string();
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(timeout, None, sender)
            .map_err(|e| format!("failed to create file watcher debouncer: {e}"))?;
        debouncer
            .watch(&dir_path, RecursiveMode::NonRecursive)
            .map_err(|e| {
                format!(
                    "failed to watch file change for {}: {e}",
                    file_path.display()
                )
            })?;
        let join_handle = std::thread::spawn(move || {
            let _keep_debouncer_alive = debouncer;
            for res in receiver {
                match res {
                    Ok(events) => {
                        let touched = events
                            .iter()
                            .any(|event| is_relevant_event(event, &file_name));
                        if touched && file_path.try_exists().is_ok_and(|exists| exists) {
                            on_change();
                        }
                    }
                    Err(_errors) => {
                        on_error();
                    }
                }
            }
            unreachable!("watcher loop exited");
        });
        return Ok(join_handle);
    }
    return inner(file_path.as_ref(), on_change, on_error, timeout);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_only_whitelisted_url_schemes() {
        for url in [
            "https://github.com/zhb2000/anki-marker",
            "http://localhost:8765",
            "HTTPS://example.com/?a=1&b=2",
            "mailto:someone@example.com",
            "x-apple.systempreferences:com.apple.LoginItems-Settings.extension",
            "ms-settings:startupapps",
        ] {
            assert!(check_url_allowed(url).is_ok(), "should allow {url}");
        }
        for url in [
            "file:///Applications/Calculator.app",
            "/Applications/Calculator.app",
            "C:\\Windows\\System32\\calc.exe",
            "javascript:alert(1)",
            "https:evil",
            " https://example.com",
            "https://example.com/\nfoo",
            "smb://host/share",
            "",
        ] {
            assert!(check_url_allowed(url).is_err(), "should refuse {url:?}");
        }
    }

    #[test]
    fn filters_watch_events_by_file_name() {
        use notify::event::{CreateKind, EventKind, Flag, ModifyKind, RenameMode};
        let name = OsStr::new("config.toml");
        let dir = std::env::temp_dir();
        // 原子保存：临时文件 rename 为目标文件，事件同时带有两个路径
        let rename = notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(dir.join(".config.toml.tmp"))
            .add_path(dir.join("config.toml"));
        assert!(is_relevant_event(&rename, name));
        let in_place = notify::Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(dir.join("config.toml"));
        assert!(is_relevant_event(&in_place, name));
        // 只涉及临时文件或同目录其它文件的事件不触发
        let temp_only = notify::Event::new(EventKind::Create(CreateKind::File))
            .add_path(dir.join(".config.toml.tmp"));
        assert!(!is_relevant_event(&temp_only, name));
        let other = notify::Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(dir.join("config.toml.bak"));
        assert!(!is_relevant_event(&other, name));
        // 事件队列溢出：无法确定变化范围，按“可能变了”处理
        let rescan = notify::Event::new(EventKind::Other).set_flag(Flag::Rescan);
        assert!(is_relevant_event(&rescan, name));
    }

    /// 原子写入后文件内容与输入逐字节一致，且不残留临时文件
    #[test]
    fn atomic_write_keeps_the_whole_content() {
        let dir = std::env::temp_dir().join("anki-marker-test-atomic-write");
        std::fs::create_dir_all(&dir).expect("failed to create the test directory");
        let path = dir.join("config.toml");
        std::fs::write(&path, "old = 1\n".repeat(1000)).expect("failed to write the old file");
        let content = "# 注释：保留“中文”与空白\n\nkey = \"a&b^c%PATH%\"\r\nother = true\n  \t\n";
        write_file_atomically(&path, content.as_bytes()).expect("failed to write atomically");
        assert_eq!(
            std::fs::read(&path).expect("failed to read back"),
            content.as_bytes()
        );
        assert!(
            !dir.join(".config.toml.tmp").exists(),
            "temp file should be renamed away"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 目标是符号链接时写入链接指向的文件，链接本身保持不变
    #[cfg(unix)]
    #[test]
    fn atomic_write_keeps_symlink() {
        let dir = std::env::temp_dir().join("anki-marker-test-atomic-symlink");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real")).expect("failed to create the test directory");
        let real = dir.join("real").join("config.toml");
        std::fs::write(&real, "old = 1\n").expect("failed to write the real file");
        let link = dir.join("config.toml");
        std::os::unix::fs::symlink(&real, &link).expect("failed to create the symlink");
        write_file_atomically(&link, b"new = 2\n").expect("failed to write atomically");
        assert!(std::fs::symlink_metadata(&link)
            .expect("failed to stat the link")
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read_to_string(&real).expect("failed to read"),
            "new = 2\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
