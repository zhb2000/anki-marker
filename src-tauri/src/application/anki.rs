use super::logics;

/// 启动 Anki 进程（非阻塞）。`anki_executable_path` 为空串时视为未指定（自动探测）。
#[tauri::command(rename_all = "snake_case")]
pub fn launch_anki(anki_executable_path: Option<String>) -> Result<(), String> {
    // 空串归一化为 None 后再传逻辑层
    let anki_executable_path = anki_executable_path.filter(|path| !path.is_empty());
    return logics::anki::launch_anki_impl(anki_executable_path.as_deref());
}

/// 检测 Anki 进程是否在运行。
#[tauri::command(rename_all = "snake_case")]
pub fn is_anki_running() -> Result<bool, String> {
    return logics::anki::is_anki_running_impl();
}
