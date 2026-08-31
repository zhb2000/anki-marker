<script setup lang="ts">
import { ref, computed, watch, nextTick, onActivated, onBeforeMount } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import * as api from '../tauri-api';

import { useSettingsStore } from '../logics/settings-store';
import {
    SETTINGS_PAGES, searchSettings,
    type SettingEntry, type SettingsPageId,
} from '../logics/settings-registry';
import * as globals from '../logics/globals';
import { effectiveTextSetting } from '../logics/config';
import { accessibilityTrusted, checkAccessibilityTrust, shortcutError } from '../logics/shortcut-status';
import { FluentInput } from '../fluent-controls';
import { ReturnButton, SettingsNavIcon } from '../components';

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

// #region 侧边栏选中指示器
/**
 * WinUI NavigationView 风格的选中指示器：一个独立于导航项的圆角竖线元素，
 * 通过 translateY 在选中项之间平移（切换时有移动动画）。
 * 定位基准是 .sidebar-scroll（position: relative），故用 offsetTop 计算。
 */

/** 导航列表容器元素（指示器的定位基准） */
const navListRef = ref<HTMLElement | null>(null);

/** 指示器垂直偏移（相对列表容器顶部，px） */
const indicatorY = ref(0);

/** 指示器高度: WinUI NavigationViewSelectionIndicatorHeight */
const INDICATOR_HEIGHT = 16;

/** 将指示器对齐到当前选中项左侧垂直居中 */
function updateIndicator() {
    const activeEl = navListRef.value?.querySelector<HTMLElement>('.nav-item.active');
    if (!activeEl) return;
    indicatorY.value = activeEl.offsetTop + (activeEl.offsetHeight - INDICATOR_HEIGHT) / 2;
}

// 路由切换 / 搜索态进出 / 首次就绪后重新对齐（搜索态下导航项整体卸载，返回时需重算）
watch(
    [() => route.path, ready, isSearching],
    () => nextTick(updateIndicator),
    { immediate: true },
);
// #endregion

// #region 子页面滚动位置记忆
/**
 * .content-area 是所有子页面共享的滚动容器：KeepAlive 保留的是组件状态，
 * 而 scrollTop 挂在容器这个 DOM 元素上，直接切换子页面会沿用上一页的偏移。
 * 与词典列表 ScrollMemory（v-show 列表各自持有滚动容器）是同类问题的不同形态，
 * 这里按页面 id 在共享容器上记忆/恢复各自的滚动位置。
 */

/** 滚动容器 .content-area */
const contentAreaRef = ref<HTMLElement | null>(null);

/** 每个子页面各自的滚动位置 */
const scrollPositions = new Map<SettingsPageId, number>();

/** 从路由路径解析子页面 id（非设置子路由路径返回 null） */
function pageIdFromPath(path: string): SettingsPageId | null {
    const segment = path.split('/')[2];
    return SETTINGS_PAGES.some(page => page.id === segment) ? (segment as SettingsPageId) : null;
}

/** 离开子页面时记住其滚动位置（pre-flush 时机: DOM 未更新，scrollTop 仍属旧页面） */
watch(() => route.path, (_newPath, oldPath) => {
    const oldId = pageIdFromPath(oldPath);
    if (contentAreaRef.value && oldId) {
        scrollPositions.set(oldId, contentAreaRef.value.scrollTop);
    }
});

/**
 * 恢复新页面的滚动位置，挂在 Transition 的 @before-enter 上。
 * 实测（KeepAlive + out-in）: before-enter 触发时新页面 DOM 尚未插入文档
 * （el.isConnected=false），容器此刻内容高度不足，直接赋值 scrollTop 会被
 * clamp 成 0；nextTick 后 DOM 已就位，赋值才能生效（此时仍早于首帧绘制，
 * 不会闪动）。
 * 若本次跳转带 ?h 锚点，子页面 useHighlight 的 scrollIntoView 在 mounted
 * 之后执行，会覆盖此恢复值，两者不冲突。
 */
function handlePageBeforeEnter() {
    const newId = pageIdFromPath(route.path);
    void nextTick(() => {
        const container = contentAreaRef.value;
        if (container && newId) {
            container.scrollTop = scrollPositions.get(newId) ?? 0;
        }
    });
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
    // 从主页返回设置页时恢复当前子页面的滚动位置（设置页整体重挂载，
    // 内部 Transition 不触发 before-enter；?h 锚点跳转交给 useHighlight 定位）
    const id = pageIdFromPath(route.path);
    if (id != null && route.query.h == null) {
        void nextTick(() => {
            const container = contentAreaRef.value;
            if (container) container.scrollTop = scrollPositions.get(id) ?? 0;
        });
    }
    // 进入设置页时从 config 全量回拷 state，同步外部修改
    store.syncFromConfig();
    // 刷新导航红点的数据源：辅助功能权限状态、Anki 内笔记模板版本
    void checkAccessibilityTrust();
    void globals.getConfig().then(config =>
        globals.fetchAndSetTemplateVersion(effectiveTextSetting('modelName', config.modelName)));
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
                <div ref="navListRef" class="sidebar-scroll">
                    <template v-if="!isSearching">
                        <!-- WinUI NavigationView 风格选中指示器: 独立元素平移到选中项 -->
                        <div class="nav-indicator" aria-hidden="true"
                            :style="{ transform: `translateY(${indicatorY}px)` }"></div>
                        <RouterLink v-for="page in visiblePages" :key="page.id" :to="`/settings/${page.id}`"
                            class="nav-item" :class="{ active: isActivePage(page.id) }">
                            <span class="nav-icon"><SettingsNavIcon :name="page.icon" /></span>
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
            <main ref="contentAreaRef" class="content-area">
                <!-- 裁剪容器: 隐藏入场动画期间页面向下溢出的部分，避免撑出滚动条 -->
                <div class="content-clip">
                    <RouterView v-slot="{ Component }">
                        <Transition name="settings-entrance" mode="out-in" @before-enter="handlePageBeforeEnter">
                            <KeepAlive>
                                <component :is="Component" />
                            </KeepAlive>
                        </Transition>
                    </RouterView>
                </div>
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
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: auto;
}

/* WinUI NavigationViewItem 规格: 36px 高胶囊、Subtle 系列背景 */
.nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 36px;
    padding: 0 12px;
    margin-bottom: 4px;
    border-radius: var(--border-radius);
    color: var(--control-text-color);
    text-decoration: none;
    font-size: 14px;
    user-select: none;
    cursor: default;
    transition: background-color 100ms ease;
}

.nav-item:hover {
    background-color: var(--nav-item-background-hover);
}

.nav-item:active {
    background-color: var(--nav-item-background-pressed);
}

/* 选中态: 背景加深一档；指示由独立元素 .nav-indicator 负责 */
.nav-item.active {
    background-color: var(--nav-item-background-selected);
}

/* 选中指示器: WinUI NavigationViewSelectionIndicator (3×16, 圆角 2, accent)。
   独立于导航项，切换时通过 translateY 平移动画移动到新选中项 */
.nav-indicator {
    position: absolute;
    left: 0;
    width: 3px;
    height: 16px;
    border-radius: 2px;
    background-color: var(--accent);
    transition: transform 250ms cubic-bezier(0.1, 0.9, 0.2, 1);
    pointer-events: none;
}

/* 导航 SVG 图标：固定宽度容器居中渲染，颜色随文字 currentColor 继承 */
.nav-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 20px;
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
    background-color: var(--nav-item-background-hover);
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

.content-clip {
    overflow: hidden;
}

/* 子页面切换: WinUI Frame 默认导航过渡 Page Refresh（EntranceNavigationTransitionInfo）——
   旧页面无退场动画、瞬时移除；新页面上移 100px 入位并淡入。
   位移/时长为该闭源动画的近似值（350ms + Fluent 减速曲线） */
.settings-entrance-enter-active {
    transition:
        transform 350ms cubic-bezier(0.1, 0.9, 0.2, 1),
        opacity 350ms cubic-bezier(0.1, 0.9, 0.2, 1);
}

.settings-entrance-enter-from {
    opacity: 0;
    transform: translateY(100px);
}

/* 旧页面瞬时让位（WinUI 无退场动画） */
.settings-entrance-leave-active {
    transition: none;
}
</style>
