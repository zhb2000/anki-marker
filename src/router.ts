import { createRouter, createWebHistory } from 'vue-router';

import Main from './views/Main.vue';
import Settings from './views/Settings.vue';

export const router = createRouter({
    history: createWebHistory(),
    routes: [
        {
            path: '/',
            name: 'Main',
            component: Main
        },
        {
            path: '/settings',
            name: 'Settings',
            component: Settings
        }
    ]
});
