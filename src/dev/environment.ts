/**
 * 当前 dev 运行环境。它决定哪些模拟项有意义、以及 “跟随环境” 类设置的默认值。
 *
 * - tauri：真实 Tauri 窗口（含 webdriver），大部分命令与网络走真实实现；
 * - browser-bridge：dev:ai 打开的浏览器页，命令转发真实 Rust，但本地副作用仍由夹具处理；
 * - browser-fixtures：dev:browser，没有 Rust，全部命令与网络由夹具处理。
 */
export type DevEnvironment = 'tauri' | 'browser-bridge' | 'browser-fixtures';

export const DEV_ENVIRONMENT_LABELS: Record<DevEnvironment, string> = {
    tauri: 'Tauri 应用',
    'browser-bridge': '浏览器 + 桥接',
    'browser-fixtures': '浏览器 + 夹具',
};

export const DEV_ENVIRONMENT_DESCRIPTIONS: Record<DevEnvironment, string> = {
    tauri: '真实应用：Tauri 运行时；界面场景、行为开关与 AI 请求会按开发者设置模拟，其余命令与网络请求走真实实现',
    'browser-bridge': '浏览器模式 + 桥接：命令与事件转发到真实 Rust；AI 请求默认由 dev 夹具模拟，剪贴板、自启动、打开程序/目录、窗口与 AnkiConnect 仍由夹具处理',
    'browser-fixtures': '浏览器模式 + 夹具：没有 Rust，命令与网络请求由 dev 夹具模拟（AI 夹具在 src/dev/mock）',
};

/** 读取当前环境。__dev.browserMode 由浏览器模式的 installBrowserMock 设置 */
export function currentDevEnvironment(): DevEnvironment {
    if (window.__dev?.browserMode !== true) {
        return 'tauri';
    }
    return window.__dev.backend === 'bridge' ? 'browser-bridge' : 'browser-fixtures';
}
