import { createRouter, createWebHistory } from 'vue-router';

import MainView from './views/MainView.vue';
import SettingsView from './views/SettingsView.vue';
import GeneralSettingsPage from './views/settings/GeneralSettingsPage.vue';
import SelectionSettingsPage from './views/settings/SelectionSettingsPage.vue';
import AnkiSettingsPage from './views/settings/AnkiSettingsPage.vue';
import AiSettingsPage from './views/settings/AiSettingsPage.vue';
import AdvancedSettingsPage from './views/settings/AdvancedSettingsPage.vue';
import AboutSettingsPage from './views/settings/AboutSettingsPage.vue';

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
            // 设置页子路由：/settings 渲染壳（SettingsView），子路由渲染各设置页面
            // 静态 import（本地桌面应用无需 code-splitting），避免首次点击 tab 时因 chunk 加载加重过渡空窗
            children: [
                {
                    path: 'general',
                    name: 'GeneralSettingsPage',
                    component: GeneralSettingsPage
                },
                {
                    path: 'selection',
                    name: 'SelectionSettingsPage',
                    component: SelectionSettingsPage
                },
                {
                    path: 'anki',
                    name: 'AnkiSettingsPage',
                    component: AnkiSettingsPage
                },
                {
                    path: 'ai',
                    name: 'AiSettingsPage',
                    component: AiSettingsPage
                },
                {
                    path: 'advanced',
                    name: 'AdvancedSettingsPage',
                    component: AdvancedSettingsPage
                },
                {
                    path: 'about',
                    name: 'AboutSettingsPage',
                    component: AboutSettingsPage
                }
            ]
        }
    ]
});
