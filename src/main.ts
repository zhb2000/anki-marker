import { createApp } from 'vue';
import 'normalize.css';
import ElementPlus from 'element-plus';
import 'element-plus/dist/index.css';

import './fluent-controls/fluent-styles.css';
import './fluent-controls/fluent-scrollbar.css';
import App from './App.vue';
import { router } from './router';
import { setupFrontendLogging } from './logics/logging';

const app = createApp(App);
setupFrontendLogging(app);
app.use(router);
app.use(ElementPlus);
app.mount('#app');
