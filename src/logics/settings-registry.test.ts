/**
 * 设置搜索元数据与真实设置卡片的一致性校验。
 *
 * 搜索只查 SETTING_ENTRIES，跳转高亮又要求 entry.id 能在页面里找到对应的
 * [data-setting-id]，也就是说元数据与页面模板是一对需要同步维护的平行列表，
 * 漏一边不会报错、只会表现为“搜不到”或“跳过去没高亮”。
 * 这里直接把设置页模板源码作为字符串读进来（import.meta.glob ?raw，无需 node 内置模块），
 * 双向核对 setting-id：任一侧多写/漏写都会失败。
 */

import { describe, expect, it } from 'vitest';

import {
    SETTING_ENTRIES, isSettingAvailable, searchSettings,
    type SettingsPageId, type SettingsPlatform,
} from './settings-registry';

/** 应用发布的三个桌面平台 */
const PLATFORMS = ['macos', 'windows', 'linux'] as const;

/** 设置页组件文件名（不含扩展名） → 路由页 id（新增设置页时必须在此登记） */
const PAGE_BY_FILE: Record<string, SettingsPageId> = {
    GeneralSettingsPage: 'general',
    SelectionSettingsPage: 'selection',
    AnkiSettingsPage: 'anki',
    AiSettingsPage: 'ai',
    AdvancedSettingsPage: 'advanced',
    AboutSettingsPage: 'about',
};

/** 各设置页组件的原始模板源码，键形如 ../../views/settings/GeneralSettingsPage.vue */
const PAGE_SOURCES = import.meta.glob<string>('../views/settings/*SettingsPage.vue', {
    query: '?raw',
    import: 'default',
    eager: true,
});

/** 从 glob 的键里取组件文件名（不含扩展名） */
function fileNameOf(path: string): string {
    return path.split('/').pop()?.replace(/\.vue$/, '') ?? '';
}

/** 各页面的卡片锚点集合，形如 "selection:wordToSentence" */
const cardAnchors = new Set<string>();

for (const [path, source] of Object.entries(PAGE_SOURCES)) {
    const pageId = PAGE_BY_FILE[fileNameOf(path)];
    if (pageId == null) {
        continue; // 未登记的页面由下面的用例单独报错
    }
    for (const match of source.matchAll(/setting-id="([^"]+)"/g)) {
        const anchor = `${pageId}:${match[1]}`;
        expect(cardAnchors.has(anchor), `同一页出现重复的 setting-id: ${anchor}`).toBe(false);
        cardAnchors.add(anchor);
    }
}

describe('设置搜索元数据', () => {
    it('设置页目录下的每个组件都已在 PAGE_BY_FILE 登记', () => {
        const files = Object.keys(PAGE_SOURCES).map(path => `${fileNameOf(path)}.vue`);
        const registered = Object.keys(PAGE_BY_FILE).map(name => `${name}.vue`);
        expect(files.sort()).toEqual(registered.sort());
    });

    it('每个搜索条目的 id 都能在所属页面找到对应卡片', () => {
        for (const entry of SETTING_ENTRIES) {
            expect(cardAnchors.has(`${entry.page}:${entry.id}`),
                `搜索条目 ${entry.id} 在 ${entry.page} 页没有对应卡片`).toBe(true);
        }
    });

    it('每张带 setting-id 的卡片都有对应的搜索条目', () => {
        const entryAnchors = new Set(SETTING_ENTRIES.map(entry => `${entry.page}:${entry.id}`));
        for (const anchor of cardAnchors) {
            expect(entryAnchors.has(anchor), `设置卡片 ${anchor} 缺少搜索条目`).toBe(true);
        }
    });

    it('搜索条目 id 全局唯一', () => {
        const ids = SETTING_ENTRIES.map(entry => entry.id);
        expect(new Set(ids).size).toBe(ids.length);
    });

    it('划词页的设置项都能搜到', () => {
        expect(searchSettings('选词取句', 'macos').map(entry => entry.id)).toContain('wordToSentence');
        expect(searchSettings('辅助功能', 'macos').map(entry => entry.id)).toContain('accessibility-permission');
    });

    it('平台专属条目只在其支持的平台出现（可见性与搜索结果一致）', () => {
        // 规格表：只有这里列出的条目才是平台限定的，其余条目所有平台都有
        // （与设置页卡片一致：菜单栏/Dock 图标仅 macOS，托盘图标仅 Windows/Linux，辅助功能权限仅 macOS）
        const macOnly = ['showMenuBarIcon', 'keepDockIconInBackground', 'accessibility-permission'];
        const expected = (id: string): readonly SettingsPlatform[] =>
            macOnly.includes(id) ? ['macos'] :
                id === 'trayIconStyle' ? ['windows', 'linux'] :
                    PLATFORMS;

        for (const entry of SETTING_ENTRIES) {
            for (const platform of PLATFORMS) {
                const shouldShow = expected(entry.id).includes(platform);
                const where = `${entry.id}@${platform}`;
                expect(isSettingAvailable(entry.id, platform), `isSettingAvailable: ${where}`).toBe(shouldShow);
                expect(searchSettings(entry.title, platform).some(result => result.id === entry.id),
                    `searchSettings: ${where}`).toBe(shouldShow);
            }
        }
    });
});
