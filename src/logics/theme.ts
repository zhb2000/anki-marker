/**
 * 主题模式：跟随系统 / 浅色 / 深色。
 *
 * - 暗色样式全部由 `<html>` 元素上的 `dark` class 驱动
 *   （fluent-styles.css 的 html.dark 变量块、element-plus 的 dark/css-vars 等）
 * - 主题模式持久化在 localStorage（首帧由 index.html 内联脚本读取，保证窗口显示前 dark class 已定）；
 *   配置文件加载完成后会用 config.theme 校正此缓存（见 globals.initAtAppStart），两者漂移时以配置文件为准
 * - 双通道检测系统主题（仅当前模式为 system 时生效）：
 *   1. Tauri 原生 `theme()` / `onThemeChanged`（主通道，系统切换后即时触发）
 *   2. `prefers-color-scheme` 媒体查询（兜底，兼容浏览器调试环境）
 *   两个通道任一触发都会应用主题，`applyTheme` 内部幂等去重。
 */

import { ref, type Ref } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { setElementTheme } from './globals';

/** 主题模式：跟随系统 / 浅色 / 深色 */
export type ThemeMode = 'system' | 'light' | 'dark';

/** localStorage 中缓存主题模式的 key（index.html 首帧内联脚本也读取此 key，修改时需同步） */
export const THEME_MODE_STORAGE_KEY = 'anki-marker:theme-mode';

/** 当前是否处于暗色模式 */
export const isDark: Ref<boolean> = ref(false);

/** 当前主题模式（模块内维护，外部通过 getThemeMode / setThemeMode 读写） */
let currentMode: ThemeMode = 'system';

/** 应用主题：切换 dark class，并刷新依赖 CSS 变量的 element-plus 色阶 */
function applyTheme(dark: boolean): void {
    if (isDark.value === dark && document.documentElement.classList.contains('dark') === dark) {
        return; // 幂等：主题未实际变化时不重复应用
    }
    isDark.value = dark;
    document.documentElement.classList.toggle('dark', dark);
    // dark class 切换后 --accent 等 CSS 变量的计算值可能变化，需重新生成 element-plus 色阶
    setElementTheme();
}

/** 读取系统当前主题，优先 Tauri 原生 API，失败时退回媒体查询 */
async function detectSystemDark(): Promise<boolean> {
    try {
        const theme = await getCurrentWindow().theme();
        if (theme !== null) {
            return theme === 'dark';
        }
    } catch {
        // 非 Tauri 环境（如浏览器里跑 vite dev）
    }
    return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

/** 读取 localStorage 中缓存的主题模式，非法值/缺省时回退 'system' */
export function getThemeMode(): ThemeMode {
    const value = localStorage.getItem(THEME_MODE_STORAGE_KEY);
    if (value === 'system' || value === 'light' || value === 'dark') {
        return value;
    }
    return 'system';
}

/**
 * 设置主题模式：记录模式并写入 localStorage，然后立即应用。
 * - system：按系统当前主题应用，并恢复跟随系统主题变化（监听回调只在 system 模式下生效）
 * - light/dark：直接应用固定值，不再响应系统主题变化
 */
export function setThemeMode(mode: ThemeMode): void {
    currentMode = mode;
    localStorage.setItem(THEME_MODE_STORAGE_KEY, mode);
    if (mode === 'system') {
        // 与 initTheme 同流程：先用媒体查询同步应用，再用 Tauri 原生 API 校正
        applyTheme(window.matchMedia('(prefers-color-scheme: dark)').matches);
        void detectSystemDark().then(dark => {
            // 异步等待期间模式可能已被再次更改，仅在仍为 system 时应用校正结果
            if (currentMode === 'system') {
                applyTheme(dark);
            }
        });
    } else {
        applyTheme(mode === 'dark');
    }
}

let initialized = false;

/**
 * 初始化主题：按当前主题模式（localStorage 缓存，与首帧一致）应用主题，并监听系统主题变化。
 * system 模式下系统设置中切换明/暗模式后 UI 即时切换，无需重启应用。
 */
export async function initTheme(): Promise<void> {
    if (initialized) {
        return;
    }
    initialized = true;
    currentMode = getThemeMode();
    if (currentMode === 'system') {
        // 先用媒体查询同步应用一次（与 index.html 内联脚本同值），保证窗口显示前 dark class 已定
        applyTheme(window.matchMedia('(prefers-color-scheme: dark)').matches);
        // 再用 Tauri 原生 API 校正（Linux 上媒体查询可能不可靠）
        const dark = await detectSystemDark();
        // 异步等待期间模式可能已被 setThemeMode 更改，仅在仍为 system 时应用校正结果
        if (currentMode === 'system') {
            applyTheme(dark);
        }
    } else {
        // 固定模式：与 index.html 内联脚本同值，通常已被 applyTheme 幂等去重
        applyTheme(currentMode === 'dark');
    }
    // 主通道：Tauri 原生系统主题变化事件（仅 system 模式下跟随）
    try {
        await getCurrentWindow().onThemeChanged(({ payload }) => {
            if (currentMode === 'system') {
                applyTheme(payload === 'dark');
            }
        });
    } catch {
        // 非 Tauri 环境
    }
    // 兜底通道：媒体查询变化事件（仅 system 模式下跟随）
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (e) => {
        if (currentMode === 'system') {
            applyTheme(e.matches);
        }
    });
}

/**
 * 显示主窗口。
 *
 * 窗口在 tauri.conf.json 中配置为初始隐藏（visible: false），
 * 避免启动过程中原生窗口或 webview 默认底色造成闪屏；
 * 前端应用主题、完成挂载后调用本函数显示窗口。
 * Rust 侧另有 3 秒超时兜底，前端异常时窗口也会被强制显示。
 */
export async function revealMainWindow(): Promise<void> {
    try {
        await getCurrentWindow().show();
    } catch {
        // 非 Tauri 环境（浏览器调试）
    }
}
