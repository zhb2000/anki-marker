import { createApp } from 'vue';
import 'normalize.css';

import './fluent-controls/fluent-styles.css';
import './fluent-controls/fluent-scrollbar.css';
import App from './App.vue';
import { router } from './router';
import { setupFrontendLogging } from './logics/logging';
import { initTheme, revealMainWindow } from './logics/theme';

// dev 功能（开发者设置、浏览器模式）：总开关关闭时这段与 src/dev 整体被编译期剔除。
// 必须在其余初始化之前完成：浏览器模式要先装好 Tauri IPC 打桩，应用才能照常启动
if (__DEV_FEATURES__) {
    // dev 工具自身出任何问题都不能阻塞应用挂载（曾因它在原生窗口里抛错导致窗口空白、无法拖动）
    try {
        await (await import('./dev')).initDevFeatures();
    } catch (error) {
        console.error('[dev] dev 功能初始化失败，已忽略，应用照常启动', error);
    }
}

// 初始化主题模式（跟随系统/浅色/深色；首帧配色由 index.html 内联脚本保证，这里异步精细接管）
void initTheme();

const app = createApp(App);
setupFrontendLogging(app);
app.use(router);
app.mount('#app');
// 窗口初始隐藏（防启动闪屏）：主题已应用、首帧挂载完成后显示
void revealMainWindow();
