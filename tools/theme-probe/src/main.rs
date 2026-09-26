//! Windows 托盘图标主题探针（诊断工具，不属于应用代码，保留用于在真机复核该功能的平台前提）
//!
//! 要回答的问题：Tauri 桌面应用（无 UWP / 无 MSIX 身份）在 Windows 上，如何可靠地
//! 感知**任务栏深浅**，从而决定托盘用白色还是黑色单色图？
//!
//! 同时测量三条路：
//!   1. WM_SETTINGCHANGE（需要顶层窗口；广播消息不会送到 HWND_MESSAGE 消息窗口，
//!      故本探针创建一个隐藏顶层窗口来接）——这是应用实际采用的触发源，故此处**不做
//!      lParam 过滤**，与实现保持一致（切换 Windows 模式/应用模式/显示缩放都会广播）
//!   2. UISettings.ColorValuesChanged（WinRT 事件）——官方文档 "WinRT APIs not
//!      supported in desktop apps" 的 Unsupported members → Events 表里明确列出该事件
//!      在桌面应用**不支持**；且其实测取值只反映应用深浅。保留此项仅为在真机上复核
//!      “为何不采用它”
//!   3. 1 秒轮询两个注册表值——兜底方案，同时作为对照基准
//!
//! 另外输出 `SM_CXSMICON` 与据此选出的单色资产档位，可直接与应用的选图逻辑对照
//! （映射见 src-tauri/src/application/windows_tray.rs 的 `tray_icon_pixel_size`）。
//!
//! 关键事实：任务栏归 `SystemUsesLightTheme`，应用归 `AppsUseLightTheme`，
//! 「自定义」模式下二者可独立。Tauri/tao 读的是后者，故不能用于托盘图标。
//!
//! 运行：`cargo run`（须在 Windows 上；macOS 侧可用 `cargo xwin check
//! --target x86_64-pc-windows-msvc` 做编译校验，但无法运行）
//! 或直接用已构建的 target/x86_64-pc-windows-msvc/release/theme-probe.exe（静态 CRT，
//! 目标机无需 Rust 环境）。

#[cfg(not(windows))]
compile_error!("theme-probe 是 Windows 专用诊断工具，请在 Windows 上构建运行");

#[cfg(windows)]
mod probe {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::OnceLock;
    use std::time::Instant;

    use windows::core::{w, IInspectable, PCWSTR};
    use windows::Foundation::TypedEventHandler;
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    use windows::Win32::Foundation::{ERROR_SUCCESS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_SINGLETHREADED};
    use windows::Win32::UI::HiDpi::{
        GetAwarenessFromDpiAwarenessContext, GetThreadDpiAwarenessContext,
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        DPI_AWARENESS_PER_MONITOR_AWARE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics,
        RegisterClassW, SetTimer, TranslateMessage, MSG, SM_CXSMICON, WNDCLASSW,
        WM_DWMCOLORIZATIONCOLORCHANGED, WM_SETTINGCHANGE, WM_TIMER, WS_OVERLAPPED,
    };

    const PERSONALIZE: PCWSTR =
        w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    const WINDOWS_VERSION: PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    const TIMER_ID: usize = 1;

    /// 进程启动时刻：日志用相对秒数，避免引入时间格式化依赖
    static START: OnceLock<Instant> = OnceLock::new();
    /// UISettings.ColorValuesChanged 触发次数（事件可能在别线程回调，故用原子量）
    static UI_SETTINGS_HITS: AtomicU32 = AtomicU32::new(0);
    /// 轮询基准：SystemUsesLightTheme / AppsUseLightTheme，u32::MAX 表示尚未读取
    static LAST_SYSTEM: AtomicU32 = AtomicU32::new(u32::MAX);
    static LAST_APPS: AtomicU32 = AtomicU32::new(u32::MAX);

    thread_local! {
        /// UISettings 实例：消息循环与窗口过程都在主线程，用 thread_local 规避 Send/Sync 问题
        static UI_SETTINGS: std::cell::RefCell<Option<UISettings>> =
            const { std::cell::RefCell::new(None) };
    }

    pub fn run() {
        // 时间戳以进程启动为零点（而非首次事件），否则首条日志恒为 +0.0s
        let _ = START.set(Instant::now());
        // 与 Tauri 应用保持同一 DPI 上下文：否则本进程（DPI 未感知）读到的
        // SM_CXSMICON 恒为 16，无法与应用（PerMonitorV2 感知）实际取到的档位对照
        let dpi_ok = set_per_monitor_dpi_aware();
        // WinRT 需要初始化套间；失败（如已被初始化为 MTA）不致命，继续跑
        let ro = unsafe { RoInitialize(RO_INIT_SINGLETHREADED) };
        print_banner(ro.is_ok(), dpi_ok);
        // 订阅必须在打印基线之前：否则基线那行的 UISettings.Background 会是“不可用”
        subscribe_ui_settings();
        report("基线", true);
        println!();

        let hwnd = create_hidden_window();
        seed_baseline();
        unsafe {
            SetTimer(Some(hwnd), TIMER_ID, 1000, None);
            message_loop();
        }
    }

    /// 声明 PerMonitorV2 DPI 感知，使 SM_CXSMICON 的读数与应用一致。
    /// 声明失败（如系统过旧）仅影响下面那行尺寸提示的保真度，不影响其余探测。
    fn set_per_monitor_dpi_aware() -> bool {
        return unsafe {
            // 已是目标状态时 SetProcessDpiAwarenessContext 会返回 ACCESS_DENIED，
            // 因此以“回读当前状态”为准，避免误报失败
            let current = GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext());
            if current == DPI_AWARENESS_PER_MONITOR_AWARE {
                return true;
            }
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_ok()
        };
    }

    /// 把当前注册表值记为轮询基准，避免启动 1 秒后误报「发生变化」
    fn seed_baseline() {
        let system = read_dword(HKEY_CURRENT_USER, PERSONALIZE, w!("SystemUsesLightTheme"))
            .unwrap_or(u32::MAX);
        let apps =
            read_dword(HKEY_CURRENT_USER, PERSONALIZE, w!("AppsUseLightTheme")).unwrap_or(u32::MAX);
        LAST_SYSTEM.store(system, Ordering::SeqCst);
        LAST_APPS.store(apps, Ordering::SeqCst);
    }

    fn print_banner(ro_ok: bool, dpi_ok: bool) {
        println!("=== Anki Marker · Windows 托盘图标主题探针 ===");
        println!();
        println!(
            "Windows: {} (build {})",
            read_string(HKEY_LOCAL_MACHINE, WINDOWS_VERSION, w!("DisplayVersion"))
                .unwrap_or_else(|| "?".into()),
            read_string(HKEY_LOCAL_MACHINE, WINDOWS_VERSION, w!("CurrentBuildNumber"))
                .unwrap_or_else(|| "?".into())
        );
        println!("WinRT 套间初始化: {}", if ro_ok { "成功" } else { "失败（忽略，仅影响第 2 项）" });
        println!(
            "SM_CXSMICON={}{} → 应用取 {} 档单色资产（mono-{{white,black}}-{}.png）",
            unsafe { GetSystemMetrics(SM_CXSMICON) },
            if dpi_ok { "" } else { "（DPI 感知声明失败，此值可能与应用不一致）" },
            icon_size_bucket(),
            icon_size_bucket()
        );
        println!();
        println!("请依次操作（设置 → 个性化 → 颜色 → 选择模式 → 自定义）：");
        println!("  1) 只切换「默认 Windows 模式」深 ⇄ 浅   ← 这一项才是任务栏");
        println!("  2) 只切换「默认 应用模式」深 ⇄ 浅");
        println!("每次切换后回到本窗口看输出。");
        println!();
        println!("判读：");
        println!("  [事件] 行出现 = 该触发源可靠，可实现「即时跟随」");
        println!("  只有 [轮询] 行 = 该触发源没送达，只能退化为低频轮询");
        println!("  [UISET] 行出现 = WinRT 事件居然触发了（官方文档称桌面应用不支持，请记录系统版本）");
        println!();
    }

    /// 订阅 UISettings.ColorValuesChanged（官方文档标注桌面应用不支持，实测用）
    fn subscribe_ui_settings() {
        let settings = match UISettings::new() {
            Ok(s) => s,
            Err(e) => {
                println!("[初始化] UISettings 创建失败：{e}（此项跳过）");
                return;
            }
        };
        let handler = TypedEventHandler::<UISettings, IInspectable>::new(|_, _| {
            UI_SETTINGS_HITS.fetch_add(1, Ordering::SeqCst);
            println!(
                "[UISET ] ColorValuesChanged 触发了（官方文档称桌面应用不支持，请记录此现象）"
            );
            Ok(())
        });
        match settings.ColorValuesChanged(&handler) {
            Ok(_token) => println!("[初始化] UISettings.ColorValuesChanged 已订阅"),
            Err(e) => println!("[初始化] 订阅 ColorValuesChanged 失败：{e}"),
        }
        // 存进 thread_local 保持存活，并在 report() 中用作「UISettings 值来源」对照
        UI_SETTINGS.with(|slot| *slot.borrow_mut() = Some(settings));
    }

    /// 读 UISettings 的背景色：用于判断它跟踪的是「Windows 模式」还是「应用模式」
    fn ui_settings_background() -> Option<String> {
        UI_SETTINGS.with(|slot| {
            let borrowed = slot.borrow();
            let settings = borrowed.as_ref()?;
            match settings.GetColorValue(UIColorType::Background) {
                Ok(color) => Some(format!("#{:02X}{:02X}{:02X}{:02X}", color.A, color.R, color.G, color.B)),
                Err(_) => None,
            }
        })
    }

    /// 创建隐藏的顶层窗口：广播消息（WM_SETTINGCHANGE）不会送到消息窗口（HWND_MESSAGE），
    /// 所以必须是一个普通顶层窗口；不设 WS_VISIBLE 即不显示。
    fn create_hidden_window() -> HWND {
        unsafe {
            let instance = GetModuleHandleW(None).expect("GetModuleHandleW 失败");
            let class = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: HINSTANCE(instance.0),
                lpszClassName: w!("AnkiMarkerThemeProbe"),
                ..Default::default()
            };
            RegisterClassW(&class);
            CreateWindowExW(
                Default::default(),
                w!("AnkiMarkerThemeProbe"),
                w!("AnkiMarkerThemeProbe"),
                WS_OVERLAPPED,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(HINSTANCE(instance.0)),
                None,
            )
            .expect("CreateWindowExW 失败")
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_SETTINGCHANGE => {
                let setting = unsafe { read_wide_string(lparam) };
                let name = setting.as_deref().unwrap_or("(null)");
                if name == "(null)" {
                    // 高对比度等场景 lParam 为 null，wParam 携带类别
                    println!("[事件] {} WM_SETTINGCHANGE wParam=0x{:04X} lParam=null", stamp(), wparam.0);
                } else {
                    println!("[事件] {} WM_SETTINGCHANGE \"{}\"", stamp(), name);
                }
                report("事件", false);
            }
            WM_DWMCOLORIZATIONCOLORCHANGED => {
                println!("[事件] {} WM_DWMCOLORIZATIONCOLORCHANGED（强调色变化）", stamp());
            }
            WM_TIMER => poll_once(),
            _ => {}
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    unsafe fn message_loop() {
        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// 1 秒轮询：仅在注册表值发生变化时输出，作为对照基准与兜底方案
    fn poll_once() {
        let system = read_dword(HKEY_CURRENT_USER, PERSONALIZE, w!("SystemUsesLightTheme"));
        let apps = read_dword(HKEY_CURRENT_USER, PERSONALIZE, w!("AppsUseLightTheme"));
        let system_key = system.unwrap_or(u32::MAX);
        let apps_key = apps.unwrap_or(u32::MAX);
        let changed = LAST_SYSTEM.swap(system_key, Ordering::SeqCst) != system_key
            || LAST_APPS.swap(apps_key, Ordering::SeqCst) != apps_key;
        if changed {
            println!(
                "[轮询] {} 注册表值发生变化（若上方紧邻没有 [事件] 行，说明事件源不可靠）",
                stamp()
            );
            report("轮询", false);
        }
    }

    /// 应用实际会选用的单色资产档位。
    /// 映射与 src-tauri/src/application/windows_tray.rs 的 `tray_icon_pixel_size` 保持一致
    /// （100% DPI = 16、150% ≈ 20/24、200% = 32）。
    fn icon_size_bucket() -> u32 {
        let cx = unsafe { GetSystemMetrics(SM_CXSMICON) };
        return match cx {
            ..=16 => 16,
            17..=20 => 20,
            21..=24 => 24,
            _ => 32,
        };
    }

    /// 依据任务栏深浅与 DPI 档位给出应用实际会加载的资产文件名
    fn suggested_asset(system: Option<u32>) -> String {
        // 键缺失按深色处理：该键随 Light 主题在 Windows 10 1903 引入，更早的系统
        // （Win10 早期 / Win7 / Win8.1）没有它，而那时的任务栏恒为深色
        let color = match system {
            Some(1) => "black",
            _ => "white",
        };
        return format!("mono-{color}-{}.png", icon_size_bucket());
    }

    /// 输出两个注册表值 + 应用会选用的资产 + UISettings 背景色
    fn report(trigger: &str, baseline: bool) {
        let system = read_dword(HKEY_CURRENT_USER, PERSONALIZE, w!("SystemUsesLightTheme"));
        let apps = read_dword(HKEY_CURRENT_USER, PERSONALIZE, w!("AppsUseLightTheme"));
        println!(
            "         SystemUsesLightTheme={}（任务栏）  AppsUseLightTheme={}（应用）  应用会选用 {}",
            describe(system),
            describe(apps),
            suggested_asset(system)
        );
        println!(
            "         UISettings.Background={}（对比上面两个值可判断它跟踪的是哪一项）",
            ui_settings_background().unwrap_or_else(|| "不可用".into())
        );
        if baseline {
            println!("         （以上为当前基线；注册表键缺失时按深色处理——1903 之前无此键，任务栏恒为深色）");
        }
        let hits = UI_SETTINGS_HITS.load(Ordering::SeqCst);
        if hits > 0 && trigger != "事件" {
            println!("         UISettings.ColorValuesChanged 累计触发 {hits} 次");
        }
    }

    fn describe(value: Option<u32>) -> &'static str {
        match value {
            Some(0) => "0/深",
            Some(_) => "1/浅",
            None => "缺",
        }
    }

    fn stamp() -> String {
        let elapsed = START.get_or_init(Instant::now).elapsed();
        format!("+{:6.1}s", elapsed.as_secs_f64())
    }

    /// 读取 WM_SETTINGCHANGE 的 lParam（指向以 NUL 结尾的宽字符串）
    unsafe fn read_wide_string(lparam: LPARAM) -> Option<String> {
        let ptr = lparam.0 as *const u16;
        if ptr.is_null() {
            return None;
        }
        let mut len = 0usize;
        while len < 128 && unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(unsafe {
            std::slice::from_raw_parts(ptr, len)
        }))
    }

    fn read_dword(root: windows::Win32::System::Registry::HKEY, key: PCWSTR, value: PCWSTR) -> Option<u32> {
        let mut data: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(
                root,
                key,
                value,
                RRF_RT_REG_DWORD,
                None,
                Some(&mut data as *mut u32 as *mut _),
                Some(&mut size),
            )
        };
        (status == ERROR_SUCCESS).then_some(data)
    }

    fn read_string(
        root: windows::Win32::System::Registry::HKEY,
        key: PCWSTR,
        value: PCWSTR,
    ) -> Option<String> {
        let mut buffer = [0u16; 128];
        let mut size = (buffer.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                root,
                key,
                value,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr() as *mut _),
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buffer[..len]))
    }
}

#[cfg(windows)]
fn main() {
    probe::run();
}
