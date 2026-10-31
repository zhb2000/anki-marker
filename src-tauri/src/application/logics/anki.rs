/// 启动 Anki 进程（spawn 非阻塞，启动后立即返回）。
///
/// `anki_executable_path` 为 None 或空串时自动探测 Anki 位置。
pub fn launch_anki_impl(anki_executable_path: Option<&str>) -> Result<(), String> {
    // 空串视为 None（前端可能传空串），统一走自动探测
    let anki_executable_path = anki_executable_path.filter(|path| !path.is_empty());

    #[cfg(target_os = "macos")]
    fn inner(anki_executable_path: Option<&str>) -> Result<(), String> {
        // 实测结论（2026-08，macOS 26 / Anki 26.x）：open -j/--hide、open -g、
        // AppleScript launch 等“隐藏/不激活”启动方式均无法阻止 Anki（Qt 应用）
        // 在启动时自行显示并激活主窗口（启动后约 1~2 秒还会二次显示/激活），
        // 故直接使用 open 常规启动，接受其抢占一次前台。
        let app = anki_executable_path.unwrap_or("Anki");
        log::info!("launching Anki via macOS open (app: {app})");
        super::utils::spawn_and_reap(std::process::Command::new("open").args(["-a", app]))
            .map_err(|e| format!("failed to launch Anki: {e}"))?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    fn inner(anki_executable_path: Option<&str>) -> Result<(), String> {
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE;
        // 路径解析顺序：用户配置路径 > Anki 默认安装位置 > 裸 anki.exe（交给 ShellExecute
        // 按 App Paths 注册表与 PATH 查找，与原先的 start 一致）
        let path = match anki_executable_path {
            Some(path) => path.to_string(),
            None => match anki_default_install_path() {
                Some(path) => path,
                None => "anki.exe".to_string(),
            },
        };
        log::info!("launching Anki via ShellExecuteW (path: {path})");
        // 最小化且不激活启动（等价于 start /min）。不经 cmd /C 转发：路径中的 & ^ % 等
        // 会被 cmd 当作元字符解释，导致启动失败甚至执行注入的命令。
        // std 的 Command 不支持设置 STARTUPINFO.wShowWindow（相关 API 尚未稳定），故用 ShellExecuteW
        return super::utils::shell_execute(&path, SW_SHOWMINNOACTIVE)
            .map_err(|e| format!("failed to launch Anki: {e}"));
    }

    #[cfg(target_os = "linux")]
    fn inner(anki_executable_path: Option<&str>) -> Result<(), String> {
        // 用户配置路径优先；启动失败时回退到自动探测
        if let Some(path) = anki_executable_path {
            match super::utils::spawn_and_reap(&mut std::process::Command::new(path)) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    log::warn!(
                        "failed to launch Anki from configured path {path}: {e}, falling back to auto detection"
                    );
                }
            }
        }
        // 子进程均由后台线程 wait 回收：否则 Anki 退出后以僵尸进程残留，仍被检测为“正在运行”
        match super::utils::spawn_and_reap(&mut std::process::Command::new("anki")) {
            Ok(_) => return Ok(()),
            // anki 命令不存在时回退到 flatpak 版 Anki（flatpak 场景下进程名仍为 anki）
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                log::info!("anki command not found, launching Anki via flatpak");
                super::utils::spawn_and_reap(
                    std::process::Command::new("flatpak").args(["run", "net.ankiweb.Anki"]),
                )
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

/// 检测 Anki 进程是否在运行（尽力而为：检测手段不可用时返回 Ok(false)，不报错）。
pub fn is_anki_running_impl() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    fn inner() -> Result<bool, String> {
        // pgrep 退出码 0 表示匹配到进程。
        // 必须带 -i：macOS 上 Anki.app 的可执行文件名是 "Anki"（大写），pgrep 默认大小写敏感，
        // 只用 -x anki 会永远匹配不到，导致“Anki 正在运行但连不上 AnkiConnect”这一判断失效。
        // macOS 经 open 交给 LaunchServices 拉起 Anki，Anki 不是本进程的子进程，不会留僵尸
        let output = match std::process::Command::new("pgrep")
            .args(["-i", "-x", "anki"])
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

    #[cfg(target_os = "linux")]
    fn inner() -> Result<bool, String> {
        // 直接扫描 /proc，按进程名（comm，大小写不敏感，flatpak 版同样为 anki）匹配，
        // 并排除僵尸（Z）与已死（X）状态：pgrep 默认不排除僵尸进程，而排除用的 -r 选项
        // 只有较新的 procps-ng 才支持
        let entries = match std::fs::read_dir("/proc") {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!("failed to read /proc to detect Anki process: {e}");
                return Ok(false);
            }
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let is_pid = file_name
                .to_str()
                .is_some_and(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()));
            if !is_pid {
                continue;
            }
            // 进程可能在枚举期间退出，读取失败直接跳过
            let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            if is_live_anki_stat(&stat) {
                return Ok(true);
            }
        }
        return Ok(false);
    }

    #[cfg(target_os = "windows")]
    fn inner() -> Result<bool, String> {
        // 用 Toolhelp 快照枚举进程，不再起 tasklist 子进程（后者通常要几百毫秒）
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
            Ok(snapshot) => snapshot,
            Err(e) => {
                log::warn!("failed to snapshot processes to detect Anki process: {e}");
                return Ok(false);
            }
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        let mut next = unsafe { Process32FirstW(snapshot, &mut entry) };
        while next.is_ok() {
            if is_anki_exe_name(&entry.szExeFile) {
                found = true;
                break;
            }
            next = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        let _ = unsafe { CloseHandle(snapshot) };
        return Ok(found);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    fn inner() -> Result<bool, String> {
        return Err("unsupported platform".to_string());
    }

    return inner();
}

/// 解析 /proc/<pid>/stat，判断是否为存活的 Anki 进程。
///
/// stat 格式为 `pid (comm) state ...`，comm 本身可能含空格与括号，因此取第一个 `(`
/// 与最后一个 `)` 之间为进程名，其后第一个字段为状态。
#[cfg(any(target_os = "linux", test))]
fn is_live_anki_stat(stat: &str) -> bool {
    let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
        return false;
    };
    if close <= open {
        return false;
    }
    let comm = &stat[open + 1..close];
    let Some(state) = stat[close + 1..].split_whitespace().next() else {
        return false;
    };
    return comm.eq_ignore_ascii_case("anki") && state != "Z" && state != "X";
}

/// PROCESSENTRY32W.szExeFile（以 NUL 结尾的 UTF-16）是否为 anki.exe（大小写不敏感）
#[cfg(any(target_os = "windows", test))]
fn is_anki_exe_name(exe_file: &[u16]) -> bool {
    let len = exe_file
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(exe_file.len());
    return String::from_utf16_lossy(&exe_file[..len]).eq_ignore_ascii_case("anki.exe");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_live_anki_from_proc_stat() {
        assert!(is_live_anki_stat(
            "1234 (anki) S 1 1234 1234 0 -1 4194560 0"
        ));
        assert!(is_live_anki_stat("1234 (Anki) R 1 1234 1234 0 -1"));
        // 僵尸 / 已死进程不算运行中
        assert!(!is_live_anki_stat("1234 (anki) Z 1 1234 1234 0 -1"));
        assert!(!is_live_anki_stat("1234 (anki) X 1 1234 1234 0 -1"));
        // 进程名含空格与括号时按最后一个右括号切分
        assert!(!is_live_anki_stat("1234 (anki) (x) S 1"));
        assert!(!is_live_anki_stat("1234 (anki-launcher) S 1"));
        assert!(!is_live_anki_stat("1234 (ankiconnect) S 1"));
        assert!(!is_live_anki_stat("garbage"));
        assert!(!is_live_anki_stat("1234 (anki)"));
    }

    #[test]
    fn matches_anki_exe_name() {
        let mut buf = [0u16; 260];
        for (i, c) in "Anki.EXE".encode_utf16().enumerate() {
            buf[i] = c;
        }
        assert!(is_anki_exe_name(&buf));
        let mut buf = [0u16; 260];
        for (i, c) in "anki.exe.bak".encode_utf16().enumerate() {
            buf[i] = c;
        }
        assert!(!is_anki_exe_name(&buf));
        assert!(!is_anki_exe_name(&[0u16; 260]));
    }
}
