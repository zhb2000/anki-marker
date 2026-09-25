// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::Manager;

mod application;

/// 用户已主动关闭主窗口（点关闭按钮被拦截为隐藏）。
/// 用于阻止启动兜底线程在用户关闭后强制弹出窗口。
static USER_CLOSED_MAIN_WINDOW: AtomicBool = AtomicBool::new(false);

/// 本次启动为“静默启动”：登录自启动（AppleScript 登录项 hidden 标志拉起，应用级
/// 隐藏状态，见 application::autostart）且配置为关闭窗口后保持后台运行，主窗口不
/// 自动显示、应用按 background-icon 配置进入后台模式。
/// 供启动兜底逻辑跳过强制显示，及前端 revealMainWindow 决定是否显示窗口。
static START_HIDDEN: AtomicBool = AtomicBool::new(false);

/// 本次启动是否为“静默启动”，供前端在主题就绪后决定是否显示主窗口。
#[tauri::command]
fn was_launched_hidden() -> bool {
    return START_HIDDEN.load(Ordering::Relaxed);
}

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
        .plugin(tauri_plugin_http::init())
        // 开机自启动（系统设置入口：设置页“登录时自动启动”）。
        // macOS 采用 AppleScript 登录项模式：条目进入系统设置 → 通用 → 登录项的
        // “登录时打开”列表，按应用名+图标显示（经 LaunchServices 解析，不依赖代码签名）；
        // LaunchAgent 模式（写 plist 指向裸二进制）在 ad-hoc 签名下会被 BTM 显示为
        // 无名可执行文件，故弃用（实测 AssociatedBundleIdentifiers 也无法补救）。
        // --hidden 不进入 argv，而是映射为登录项的 hidden 标志：登录拉起时应用处于
        // NSApplication 级隐藏状态，静默启动判定见 setup 阶段（application::autostart）。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::AppleScript,
            Some(vec!["--hidden"]),
        ));

    // 全局快捷键（划词录入句子），目前仅支持 macOS
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, _shortcut, event| {
                if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                    application::shortcut::on_shortcut_pressed(app.clone());
                }
            })
            .build(),
    );

    // W3C WebDriver server at http://127.0.0.1:4445, for automated debugging/testing only.
    // Enabled via `--features webdriver`; NEVER enable for release builds.
    #[cfg(feature = "webdriver")]
    let builder = builder.plugin(tauri_plugin_webdriver::init());

    // macOS：点关闭按钮的行为可配置（keep-running-on-close）——
    // 默认仅隐藏窗口、应用保持后台运行，由点击 Dock/菜单栏图标或划词快捷键再次唤起；
    // 配置为不保持运行时则直接退出应用；其他平台维持默认行为（关闭窗口即退出应用）。
    #[cfg(target_os = "macos")]
    let builder = builder.on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            if application::menubar::keep_running_on_close(window.app_handle()) {
                // 保持运行：拦截关闭，仅隐藏窗口
                USER_CLOSED_MAIN_WINDOW.store(true, Ordering::Relaxed);
                let _ = window.hide();
                api.prevent_close();
                // 进入后台模式，按配置显示 Dock/菜单栏图标
                application::menubar::on_main_window_hidden(window.app_handle());
            } else {
                // 不保持运行：直接退出应用
                api.prevent_close();
                window.app_handle().exit(0);
            }
        }
    });

    let app = builder
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
            app.manage(application::shortcut::PendingSentence::new());

            // 注册配置中设置的划词全局快捷键（仅 macOS）
            #[cfg(target_os = "macos")]
            application::shortcut::update_from_config(app.handle());

            // 托盘/Dock 图标菜单：注册共用的菜单事件处理，并安装 Dock 图标菜单（仅 macOS）
            #[cfg(target_os = "macos")]
            {
                application::menubar::register_menu_event_handler(app.handle());
                application::menubar::install_dock_menu(app.handle());
            }

            // 静默启动判定（仅 macOS）：AppleScript 登录项带 hidden 标志拉起时，
            // 进程自启动起即处于 NSApplication 级隐藏状态（argv 无任何标记，无法从
            // 启动参数识别，见 application::autostart 模块文档）。检测到即解除应用级
            // 隐藏（不激活、不置前，保证后续托盘/Dock/快捷键唤起路径正常）；
            // 配置为关闭窗口后保持后台运行时，主窗口保持隐藏并按 background-icon 配置
            // 进入后台模式（保证菜单栏/Dock 图标可见，否则用户将无法唤起应用），
            // 否则照常显示主窗口。其他平台不支持后台常驻，一律正常显示主窗口。
            #[cfg(target_os = "macos")]
            let start_hidden = {
                let launched_hidden = application::autostart::is_app_hidden();
                if launched_hidden {
                    application::autostart::unhide_app_without_activation();
                }
                launched_hidden && application::menubar::keep_running_on_close(app.handle())
            };
            // TODO(跨平台托盘)：Windows/Linux 实现托盘图标 + close-to-tray 后，
            // 静默启动可平移过去：载体天然支持传参（注册表 Run 值 / .desktop 的 Exec
            // 均保留 argv），判定改回 std::env::args() 检查 --hidden 即可（比 macOS 的
            // NSApp 隐藏态检测更简单）；其余复用现有 START_HIDDEN 流程（前端
            // revealMainWindow 判定本就跨平台）。前置条件：menubar.rs 托盘跨平台化
            // （Win/Linux 仅“打开/退出”两项）、keepRunningOnClose 开放三端、
            // backgroundIcon 设置 UI 裁剪“Dock 栏图标”选项；Linux 打包需声明
            // libappindicator 系统依赖。文案同步升级为 macOS 同款“后台常驻”句式。
            #[cfg(not(target_os = "macos"))]
            let start_hidden = false;
            START_HIDDEN.store(start_hidden, Ordering::Relaxed);
            if start_hidden {
                application::menubar::on_main_window_hidden(app.handle());
            }

            // 防启动闪屏的兜底：窗口初始隐藏（tauri.conf.json 中 visible: false），
            // 正常由前端应用主题后调用 show() 显示；
            // 若前端异常迟迟未显示窗口，3 秒后强制显示，避免应用“隐形”。
            // 用户已主动关闭窗口时跳过，防止把刚隐藏的窗口又弹出；
            // 静默启动时主窗口本就应保持隐藏，同样跳过。
            if !start_hidden {
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if USER_CLOSED_MAIN_WINDOW.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.show();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            application::anki::launch_anki,
            application::anki::is_anki_running,
            application::config::read_config,
            application::config::commit_config,
            application::config::config_path,
            application::config::is_portable,
            application::config::show_in_explorer,
            application::config::open_filepath,
            application::config::open_in_browser,
            application::config::start_config_watcher,
            application::config::rust_in_release,
            was_launched_hidden,
            application::dict::search_collins,
            application::dict::search_oxford,
            application::dict::get_word_base,
            application::dict::sanitize_filename,
            application::shortcut::take_pending_sentence,
            application::shortcut::is_accessibility_trusted,
            application::shortcut::request_accessibility_trust,
            application::shortcut::get_shortcut_registration,
            application::window::set_window_background,
            application::window::set_dialog_mask,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // macOS：主窗口被隐藏（点关闭按钮）后点击 Dock 图标，系统不会自动恢复窗口，
        // 在此显示并聚焦；若尚有可见窗口，交给系统默认的激活行为即可。
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen {
            has_visible_windows,
            ..
        } = event
        {
            if !has_visible_windows {
                if let Err(error) = application::shortcut::show_and_focus_main_window(app_handle) {
                    log::warn!("failed to show main window on dock click: {error}");
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app_handle, event);
        }
    });
}
