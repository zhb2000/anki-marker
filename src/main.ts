import { createApp } from 'vue';
import 'normalize.css';

import './fluent-controls/fluent-styles.css';
import './fluent-controls/fluent-scrollbar.css';
import App from './App.vue';
import { router } from './router';

const app = createApp(App);
app.use(router);
app.mount('#app');
