// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::Manager;

mod application;

fn main() {
    // 统一日志：JS 侧日志经 IPC 转发到 Rust 后，与 Rust 日志走同一套 target。
    // debug：stdout + 日志文件；release：仅日志文件且只记 Warn 以上。
    #[cfg(debug_assertions)]
    let log_builder = tauri_plugin_log::Builder::new().targets([
        tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
        tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: None }),
    ]);
    #[cfg(not(debug_assertions))]
    let log_builder = tauri_plugin_log::Builder::new()
        .clear_targets()
        .targets([tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::LogDir { file_name: None },
        )])
        .level(log::LevelFilter::Warn);

    let builder = tauri::Builder::default()
        .plugin(log_builder.build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init());

    // W3C WebDriver server at http://127.0.0.1:4445, for automated debugging/testing only.
    // Enabled via `--features webdriver`; NEVER enable for release builds.
    #[cfg(feature = "webdriver")]
    let builder = builder.plugin(tauri_plugin_webdriver::init());

    builder
        .setup(|app| {
            let portable = application::config::Portable::new()?;
            app.manage(portable);
            app.manage(application::config::ConfigPath::new(
                portable.0,
                app.path(),
            )?);
            app.manage(application::config::IsWatching::new());
            app.manage(application::dict::DictPath::new(portable.0, app.path())?);
            app.manage(Mutex::new(None::<Connection>));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            application::config::read_config,
            application::config::commit_config,
            application::config::config_path,
            application::config::is_portable,
            application::config::show_in_explorer,
            application::config::open_filepath,
            application::config::open_in_browser,
            application::config::start_config_watcher,
            application::config::rust_in_release,
            application::dict::search_collins,
            application::dict::search_oxford,
            application::dict::get_word_base,
            application::dict::sanitize_filename,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
