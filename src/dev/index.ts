/**
 * dev 功能的启动入口：只在 `__DEV_FEATURES__` 为真时被 main.ts 动态导入，
 * release 构建里整个 src/dev 目录都不会进入产物。
 */
import { installDevInterceptors } from './intercept';
import { installInspector, installInvokeTrace } from './inspect';
import { initDevStore } from './store';

export async function initDevFeatures(): Promise<void> {
    await initDevStore();
    installInspector();
    // 没有 Tauri 运行时（普通浏览器打开 dev server）→ 进入浏览器模式
    if (!('__TAURI_INTERNALS__' in window)) {
        const { installBrowserMock } = await import('./browser/install');
        await installBrowserMock();
    }
    // IPC 就绪之后再包装 invoke，记录每次命令调用（真实 Tauri 窗口里做不到，见 installInvokeTrace）
    if (!installInvokeTrace()) {
        console.info('[dev] 当前环境不能包装 invoke，已跳过命令调用轨迹（浏览器模式下可用）');
    }
    // 业务侧薄包装的拦截点：HTTP 的 AI 夹具与 read_config / commit_config 的模拟 AI 配置
    installDevInterceptors();
}

export { devSettings, devOverridden } from './state';
export { setDevSetting, resetDevSetting, resetAllDevSettings } from './store';
