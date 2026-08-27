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
            component: SettingsView,
            redirect: '/settings/general',
            // 设置页子路由：/settings 渲染壳（SettingsView），子路由渲染各设置页面（懒加载）
            children: [
                {
                    path: 'general',
                    name: 'GeneralSettingsPage',
                    component: () => import('./views/settings/GeneralSettingsPage.vue')
                },
                {
                    path: 'selection',
                    name: 'SelectionSettingsPage',
                    component: () => import('./views/settings/SelectionSettingsPage.vue')
                },
                {
                    path: 'anki',
                    name: 'AnkiSettingsPage',
                    component: () => import('./views/settings/AnkiSettingsPage.vue')
                },
                {
                    path: 'ai',
                    name: 'AiSettingsPage',
                    component: () => import('./views/settings/AiSettingsPage.vue')
                },
                {
                    path: 'advanced',
                    name: 'AdvancedSettingsPage',
                    component: () => import('./views/settings/AdvancedSettingsPage.vue')
                },
                {
                    path: 'about',
                    name: 'AboutSettingsPage',
                    component: () => import('./views/settings/AboutSettingsPage.vue')
                }
            ]
        }
    ]
});
