use super::logics;

/// 启动 Anki 进程（非阻塞）。`anki_executable_path` 为空串时视为未指定（自动探测）。
///
/// async + 阻塞线程池：起子进程（Windows 上为 ShellExecute）不能占住主线程
#[tauri::command(rename_all = "snake_case")]
pub async fn launch_anki(anki_executable_path: Option<String>) -> Result<(), String> {
    // 空串归一化为 None 后再传逻辑层
    let anki_executable_path = anki_executable_path.filter(|path| !path.is_empty());
    return logics::utils::run_blocking(move || {
        logics::anki::launch_anki_impl(anki_executable_path.as_deref())
    })
    .await;
}

/// 检测 Anki 进程是否在运行。
///
/// async + 阻塞线程池：前端连不上 AnkiConnect 时会周期性调用，同步命令会在主线程执行
/// 进程检测（macOS 上起 pgrep 子进程），期间冻结界面与托盘
#[tauri::command(rename_all = "snake_case")]
pub async fn is_anki_running() -> Result<bool, String> {
    return logics::utils::run_blocking(logics::anki::is_anki_running_impl).await;
}
