// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::Manager;

mod application;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
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
