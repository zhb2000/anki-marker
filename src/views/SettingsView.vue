<script setup lang="ts">
import { ref, computed, onActivated, onBeforeMount } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import * as api from '../tauri-api';

import { useSettingsStore } from '../logics/settings-store';
import {
    SETTINGS_PAGES, searchSettings,
    type SettingEntry, type SettingsPageId,
} from '../logics/settings-registry';
import * as globals from '../logics/globals';
import { accessibilityTrusted, checkAccessibilityTrust, shortcutError } from '../logics/shortcut-status';
import { FluentInput } from '../fluent-controls';
import { ReturnButton } from '../components';

const router = useRouter();
const route = useRoute();
const store = useSettingsStore();

/** init 完成前不渲染内容区（等价原 pageInitialized，避免闪烁默认值） */
const ready = store.ready;

/** 是否为 macOS（划词设置页仅支持 macOS，非 macOS 隐藏对应导航项） */
const isMacOS = api.os.type() === 'macos';

/** 当前平台需要显示的导航页（macOnly 项在非 macOS 不渲染） */
const visiblePages = computed(() => SETTINGS_PAGES.filter(page => !page.macOnly || isMacOS));

/** 页面 id → 页面标题，用于搜索结果条目展示所属页名 */
const pageTitles = Object.fromEntries(
    SETTINGS_PAGES.map(page => [page.id, page.title])
) as Record<SettingsPageId, string>;

/** 判断导航项是否为当前页（active 判断用 route.path） */
function isActivePage(pageId: SettingsPageId): boolean {
    return route.path === `/settings/${pageId}`;
}

/**
 * 导航项是否需要显示关注红点：
 * - 关于：应用更新可用，或 Anki 内笔记模板有更新
 * - 划词：辅助功能权限未授权，或全局快捷键注册失败（冲突）
 */
function hasAttentionBadge(pageId: SettingsPageId): boolean {
    if (pageId === 'about') {
        return globals.appUpdateAvailable.value || globals.templateUpdateAvailable.value;
    }
    if (pageId === 'selection') {
        return accessibilityTrusted.value === false || shortcutError.value != null;
    }
    return false;
}

// #region 搜索
/** 搜索框内容 */
const searchQuery = ref('');

/** 搜索结果（空 query 时 searchSettings 返回 []） */
const searchResults = computed(() => searchSettings(searchQuery.value));

/** 是否处于搜索态（输入非空时左栏导航替换为搜索结果列表） */
const isSearching = computed(() => searchQuery.value.trim().length > 0);

/** 点击搜索结果：跳转所属页并带上高亮锚点 query，然后清空搜索框恢复导航 */
function handleResultClick(entry: SettingEntry) {
    void router.push({ path: `/settings/${entry.page}`, query: { h: entry.id } });
    searchQuery.value = '';
}
// #endregion

/** 点击返回按钮时先保存设置再返回主页（用 push 而非 back，避免在分类间倒走） */
async function handleReturnClick() {
    await store.flush();
    await router.push('/');
}

// 由于使用了 KeepAlive 不销毁页面，所以 onBeforeMount 只会执行一次
onBeforeMount(async () => {
    await store.init();
});

onActivated(() => {
    // 进入设置页时从 config 全量回拷 state，同步外部修改
    store.syncFromConfig();
    // 刷新导航红点的数据源：辅助功能权限状态、Anki 内笔记模板版本
    void checkAccessibilityTrust();
    void globals.getConfig().then(config => globals.fetchAndSetTemplateVersion(config.modelName));
});
</script>

<template>
    <div class="main-window">
        <div class="title-bar">
            <ReturnButton style="margin-right: 8px;" @click="handleReturnClick" />
            <h1 style="display: inline-block;">设置</h1>
        </div>
        <div class="body-area" v-if="ready">
            <aside class="sidebar">
                <FluentInput class="search-input" placeholder="搜索设置" v-model="searchQuery" clearable
                    @keydown.esc="searchQuery = ''" />
                <div class="sidebar-scroll">
                    <template v-if="!isSearching">
                        <RouterLink v-for="page in visiblePages" :key="page.id" :to="`/settings/${page.id}`"
                            class="nav-item" :class="{ active: isActivePage(page.id) }">
                            <span class="nav-icon">{{ page.icon }}</span>
                            <span>{{ page.title }}</span>
                            <span v-if="hasAttentionBadge(page.id)" class="nav-dot" title="有需要关注的项"></span>
                        </RouterLink>
                    </template>
                    <template v-else>
                        <div v-for="entry in searchResults" :key="entry.id" class="result-item"
                            @click="handleResultClick(entry)">
                            <div class="result-title-line">
                                <span class="result-title">{{ entry.title }}</span>
                                <span class="result-page">{{ pageTitles[entry.page] }}</span>
                            </div>
                            <div class="result-desc" v-if="entry.description">{{ entry.description }}</div>
                        </div>
                        <div class="no-result" v-if="searchResults.length === 0">无匹配设置</div>
                    </template>
                </div>
            </aside>
            <main class="content-area">
                <RouterView v-slot="{ Component }">
                    <Transition name="settings-fade" mode="out-in">
                        <KeepAlive>
                            <component :is="Component" />
                        </KeepAlive>
                    </Transition>
                </RouterView>
            </main>
        </div>
    </div>
</template>

<style scoped>
.main-window {
    /* 左右两栏各自独立滚动，整页不滚动 */
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background-color: var(--window-background);
    height: 100vh;
    user-select: none;
}

.title-bar {
    top: 0;
    position: sticky;
    /* 提升层级，避免内容区中由 mask-image 等属性创建的层叠上下文（如 ResetButton 图标）穿透到标题栏之上 */
    z-index: 1;
    display: flex;
    align-items: center;
    flex-shrink: 0;
    padding: 32px 20px;
    background-color: var(--window-background);
}

.body-area {
    display: flex;
    flex: 1;
    min-height: 0;
}

.sidebar {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    width: 180px;
    padding: 0 8px 8px 12px;
}

.search-input {
    flex-shrink: 0;
    width: 100%;
    height: 32px;
    margin-bottom: 8px;
}

.sidebar-scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
}

.nav-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    margin-bottom: 2px;
    border-radius: var(--border-radius);
    color: var(--control-text-color);
    text-decoration: none;
    font-size: 14px;
    user-select: none;
    cursor: default;
}

.nav-item:hover {
    background-color: var(--control-background-hover);
}

/* 选中态：--accent 文字 + 左侧 3px 指示条（inset 阴影实现，避免布局位移） */
.nav-item.active {
    color: var(--accent);
    box-shadow: inset 3px 0 0 var(--accent);
}

/* 图标为 Unicode 符号，统一字号容器渲染，保证对齐与暗色可读 */
.nav-icon {
    flex-shrink: 0;
    width: 20px;
    font-size: 14px;
    line-height: 20px;
    text-align: center;
}

/* 关注红点：右对齐的小圆点，提示该分类下有需要用户关注的项（更新/权限/快捷键冲突） */
.nav-dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    margin-left: auto;
    border-radius: 50%;
    background-color: var(--critical-fill-color);
}

.result-item {
    padding: 6px 8px;
    margin-bottom: 2px;
    border-radius: var(--border-radius);
    user-select: none;
    cursor: default;
}

.result-item:hover {
    background-color: var(--control-background-hover);
}

.result-title-line {
    display: flex;
    align-items: baseline;
    gap: 6px;
}

.result-title {
    font-size: 14px;
}

/* 所属页名小字弱化 */
.result-page {
    flex-shrink: 0;
    font-size: 12px;
    opacity: 0.6;
}

.result-desc {
    font-size: 12px;
    opacity: 0.6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}

.no-result {
    padding: 6px 8px;
    font-size: 14px;
    opacity: 0.6;
    user-select: none;
    cursor: default;
}

.content-area {
    flex: 1;
    min-width: 0;
    overflow: auto;
    padding: 24px;
    padding-top: 0;
}

h1 {
    margin: 0;
    padding: 0;
    font-size: 32px;
    font-weight: normal;
    line-height: 32px;
    user-select: none;
    cursor: default;
}

/* 子页面切换的轻 fade 动画（150ms opacity），不沿用顶层 slide 动画 */
.settings-fade-enter-active,
.settings-fade-leave-active {
    transition: opacity 150ms ease;
}

.settings-fade-enter-from,
.settings-fade-leave-to {
    opacity: 0;
}
</style>
