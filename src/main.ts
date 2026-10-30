import { createApp } from 'vue';
import 'normalize.css';
import ElementPlus from 'element-plus';
// 只引入仍在使用的组件样式：项目已把大部分 element-plus 组件换成了自绘的 fluent-controls，
// 剩 ElMessage（toast）与 ElBadge（更新红点）。全量 index.css 有 350KB+，绝大多数用不到。
import 'element-plus/theme-chalk/base.css';
import 'element-plus/theme-chalk/el-message.css';
import 'element-plus/theme-chalk/el-badge.css';
import 'element-plus/theme-chalk/dark/css-vars.css';

import './fluent-controls/fluent-styles.css';
import './fluent-controls/fluent-scrollbar.css';
import App from './App.vue';
import { router } from './router';
import { setupFrontendLogging } from './logics/logging';
import { initTheme, revealMainWindow } from './logics/theme';

// 初始化主题模式（跟随系统/浅色/深色；首帧配色由 index.html 内联脚本保证，这里异步精细接管）
void initTheme();

const app = createApp(App);
setupFrontendLogging(app);
app.use(router);
app.use(ElementPlus);
app.mount('#app');
// 窗口初始隐藏（防启动闪屏）：主题已应用、首帧挂载完成后显示
void revealMainWindow();
