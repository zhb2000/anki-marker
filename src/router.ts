import { createRouter, createWebHistory } from 'vue-router';

import MainView from './views/MainView.vue';
import SettingsView from './views/SettingsView.vue';

export const router = createRouter({
    history: createWebHistory(),
    routes: [
        {
            path: '/',
            name: 'MainView',
            component: MainView
        },
        {
            path: '/settings',
            name: 'SettingsView',
            component: SettingsView
        }
    ]
});
