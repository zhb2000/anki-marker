use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::new_debouncer;

pub fn show_in_explorer(path: impl AsRef<Path>) -> Result<(), String> {
    fn inner(path: &Path) -> Result<(), String> {
        let path = path
            .to_str()
            .ok_or("path is not valid utf-8")?
            .replace('/', std::path::MAIN_SEPARATOR_STR)
            .replace('\\', std::path::MAIN_SEPARATOR_STR);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            std::process::Command::new("cmd")
                .args(&["/C", "explorer", "/select,", &path])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .args(&["-R", &path])
                .output()
                .map_err(|e| e.to_string())?;
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            return Err(format!(
                "show_in_explorer is not implemented on this platform: {}",
                std::env::consts::OS
            ));
        }
        return Ok(());
    }
    return inner(path.as_ref());
}

pub fn current_exe_dir() -> Result<PathBuf, String> {
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("failed to get current exe path: {}", e.to_string()))?;
    let exe_dir = exe_path
        .parent()
        .ok_or("failed to get current exe directory")?;
    return Ok(exe_dir.to_path_buf());
}

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
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(timeout, None, sender)
            .map_err(|e| format!("failed to create file watcher debouncer: {e}"))?;
        debouncer
            .watcher()
            .watch(Path::new(file_path), RecursiveMode::NonRecursive)
            .map_err(|e| {
                format!(
                    "failed to watch file change for {}: {e}",
                    file_path.display()
                )
            })?;
        let file_path = file_path.to_path_buf();
        let join_handle = std::thread::spawn(move || {
            let _keep_debouncer_alive = debouncer;
            for res in receiver {
                match res {
                    Ok(_events) => {
                        if file_path.try_exists().is_ok_and(|exists| exists) {
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
