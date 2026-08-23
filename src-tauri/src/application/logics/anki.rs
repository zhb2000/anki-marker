/// 启动 Anki 进程（spawn 非阻塞，启动后立即返回）。
///
/// `anki_executable_path` 为 None 或空串时自动探测 Anki 位置。
pub fn launch_anki_impl(anki_executable_path: Option<&str>) -> Result<(), String> {
    // 空串视为 None（前端可能传空串），统一走自动探测
    let anki_executable_path = anki_executable_path.filter(|path| !path.is_empty());

    #[cfg(target_os = "macos")]
    fn inner(anki_executable_path: Option<&str>) -> Result<(), String> {
        // 实测结论（2026-08，macOS 26 / Anki 26.x）：open -j/--hide、open -g、
        // AppleScript launch 等「隐藏/不激活」启动方式均无法阻止 Anki（Qt 应用）
        // 在启动时自行显示并激活主窗口（启动后约 1~2 秒还会二次显示/激活），
        // 故直接使用 open 常规启动，接受其抢占一次前台。
        let app = anki_executable_path.unwrap_or("Anki");
        log::info!("launching Anki via macOS open (app: {app})");
        std::process::Command::new("open")
            .args(["-a", app])
            .spawn()
            .map_err(|e| format!("failed to launch Anki: {e}"))?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    fn inner(anki_executable_path: Option<&str>) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        // 路径解析顺序：用户配置路径 > Anki 默认安装位置 > 裸 anki.exe（交给 start 在 PATH 中查找）
        let path = match anki_executable_path {
            Some(path) => path.to_string(),
            None => match anki_default_install_path() {
                Some(path) => path,
                None => "anki.exe".to_string(),
            },
        };
        log::info!("launching Anki via Windows cmd start (path: {path})");
        // start 的第一个引号参数是窗口标题占位，/min 最小化启动；
        // cmd 命令本身加 CREATE_NO_WINDOW 防止闪出控制台窗口
        std::process::Command::new("cmd")
            .args(["/C", "start", "", "/min", path.as_str()])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("failed to launch Anki: {e}"))?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    fn inner(anki_executable_path: Option<&str>) -> Result<(), String> {
        // 用户配置路径优先；启动失败时回退到自动探测
        if let Some(path) = anki_executable_path {
            match std::process::Command::new(path).spawn() {
                Ok(_) => return Ok(()),
                Err(e) => {
                    log::warn!(
                        "failed to launch Anki from configured path {path}: {e}, falling back to auto detection"
                    );
                }
            }
        }
        match std::process::Command::new("anki").spawn() {
            Ok(_) => return Ok(()),
            // anki 命令不存在时回退到 flatpak 版 Anki（flatpak 场景下进程名仍为 anki）
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                log::info!("anki command not found, launching Anki via flatpak");
                std::process::Command::new("flatpak")
                    .args(["run", "net.ankiweb.Anki"])
                    .spawn()
                    .map_err(|e2| format!("failed to launch Anki: {e}; flatpak fallback: {e2}"))?;
                return Ok(());
            }
            Err(e) => return Err(format!("failed to launch Anki: {e}")),
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    fn inner(_anki_executable_path: Option<&str>) -> Result<(), String> {
        return Err("unsupported platform".to_string());
    }

    return inner(anki_executable_path);
}

/// Windows 下 Anki 的默认安装位置：%LOCALAPPDATA%\Programs\Anki\Anki.exe
#[cfg(target_os = "windows")]
fn anki_default_install_path() -> Option<String> {
    let local_app_data = std::env::var("LOCALAPPDATA").ok()?;
    let path = std::path::Path::new(&local_app_data)
        .join("Programs")
        .join("Anki")
        .join("Anki.exe");
    if matches!(path.try_exists(), Ok(true)) {
        return Some(path.to_string_lossy().into_owned());
    }
    return None;
}

/// 检测 Anki 进程是否在运行（尽力而为：检测命令不可用时返回 Ok(false)，不报错）。
pub fn is_anki_running_impl() -> Result<bool, String> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn inner() -> Result<bool, String> {
        // pgrep 退出码 0 表示匹配到进程；flatpak 安装的 Anki 进程名同样为 anki，仍可匹配
        let output = match std::process::Command::new("pgrep")
            .args(["-x", "anki"])
            .output()
        {
            Ok(output) => output,
            Err(e) => {
                log::warn!("failed to run pgrep to detect Anki process: {e}");
                return Ok(false);
            }
        };
        return Ok(output.status.success());
    }

    #[cfg(target_os = "windows")]
    fn inner() -> Result<bool, String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        // tasklist 无匹配进程时退出码也为 0，因此依据 stdout 内容判断
        let output = match std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq anki.exe", "/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            Ok(output) => output,
            Err(e) => {
                log::warn!("failed to run tasklist to detect Anki process: {e}");
                return Ok(false);
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
        return Ok(stdout.contains("anki.exe"));
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    fn inner() -> Result<bool, String> {
        return Err("unsupported platform".to_string());
    }

    return inner();
}

