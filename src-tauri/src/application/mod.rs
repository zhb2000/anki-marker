pub mod anki;
pub mod autostart;
/// 划词捕获编排：互斥、代次、后台重试补发与暂存（平台取词见 logics::selected_text）
pub mod capture_flow;
pub mod config;
pub mod dict;
/// 开发桥接：浏览器里的前端调用真实 Rust 命令（仅 dev-bridge feature，禁止进入 release）
#[cfg(feature = "dev-bridge")]
pub mod dev_bridge;
pub mod logics;
pub mod menubar;
pub mod shortcut;
pub mod window;
/// Windows 侧托盘图标平台探测：任务栏深浅、图标尺寸、深浅变更监听
#[cfg(target_os = "windows")]
pub mod windows_tray;
