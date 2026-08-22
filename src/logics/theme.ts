/**
 * 跟随系统的暗色模式。
 *
 * - 暗色样式全部由 `<html>` 元素上的 `dark` class 驱动
 *   （fluent-styles.css 的 html.dark 变量块、element-plus 的 dark/css-vars 等）
 * - 双通道检测系统主题：
 *   1. Tauri 原生 `theme()` / `onThemeChanged`（主通道，系统切换后即时触发）
 *   2. `prefers-color-scheme` 媒体查询（兜底，兼容浏览器调试环境）
 *   两个通道任一触发都会应用主题，`applyTheme` 内部幂等去重。
 */

import { ref, type Ref } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { setElementTheme } from './globals';

/** 当前是否处于暗色模式 */
export const isDark: Ref<boolean> = ref(false);

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

let initialized = false;

/**
 * 初始化主题：读取系统当前主题并监听系统主题变化。
 * 系统设置中切换明/暗模式后 UI 即时切换，无需重启应用。
 */
export async function initThemeFollowSystem(): Promise<void> {
    if (initialized) {
        return;
    }
    initialized = true;
    // 先用媒体查询同步应用一次（与 index.html 内联脚本同值），保证窗口显示前 dark class 已定
    applyTheme(window.matchMedia('(prefers-color-scheme: dark)').matches);
    // 再用 Tauri 原生 API 校正（Linux 上媒体查询可能不可靠）
    applyTheme(await detectSystemDark());
    // 主通道：Tauri 原生系统主题变化事件
    try {
        await getCurrentWindow().onThemeChanged(({ payload }) => {
            applyTheme(payload === 'dark');
        });
    } catch {
        // 非 Tauri 环境
    }
    // 兜底通道：媒体查询变化事件
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (e) => {
        applyTheme(e.matches);
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
