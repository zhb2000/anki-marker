import { createApp } from 'vue';
import 'normalize.css';

import './fluent-controls/fluent-styles.css';
import './fluent-controls/fluent-scrollbar.css';
import App from './App.vue';
import { router } from './router';

createApp(App).use(router).mount('#app');
