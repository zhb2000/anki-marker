/**
 * 搜索跳转高亮：按路由 query 参数 h 定位设置卡片并闪烁提示。
 *
 * 触发时机（页面随 shell 的 KeepAlive 常驻，三处互补）：
 * - onMounted：首次挂载时 URL 已带 h（如深链接进入）
 * - onActivated：KeepAlive 重新激活时 query.h 可能是在失活期间由搜索跳转设置的
 * - watch(route.query.h)：页面驻留期间同一页面内重复点击不同搜索结果
 *
 * 处理完成后 router.replace 清掉 query.h（保留其他 query），防止激活/再次进入时重复触发。
 * 找不到目标元素时静默忽略（目标可能是条件渲染的卡片，如平台门控的设置组、未设置快捷键时的权限卡）。
 * 注：被开关关闭的功能子配置项（如 LLM 配置）采用禁用而非隐藏，卡片始终在 DOM 中，锚点不受影响。
 *
 * 只有“当前路由就是本页、且本页处于激活状态”时才处理（含清除 h）：各页面都被 KeepAlive 常驻，
 * 跳到别的页面时，当前页与缓存中各页的 watch 会先触发；而 out-in 过渡下目标页要等离场结束才插入 DOM，
 * 它们若此时找不到元素就清掉 h，目标页挂载/激活时读到的 h 已经没了，高亮与滚动定位随之失效。
 */

import { nextTick, onActivated, onDeactivated, onMounted, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';

/** 高亮闪烁时长（毫秒），略长于 FluentSettingCard 的 1.5s 脉冲动画，保证动画播完再移除 class */
const HIGHLIGHT_FLASH_DURATION_MS = 1600;

export function useHighlight(): void {
    const route = useRoute();
    const router = useRouter();
    // 页面 setup 时的路由就是渲染本页的那条路由（父级重定向在渲染前已完成），记下作为本页路径
    const ownPath = route.path;
    /** 本页是否处于激活状态（KeepAlive 缓存中的页面为 false） */
    let active = false;

    /** 清除 query 中的 h 参数（保留其他 query），用 replace 避免留下历史记录 */
    function clearHighlightQuery(): void {
        const query = { ...route.query };
        delete query.h;
        void router.replace({ query });
    }

    async function tryHighlight(): Promise<void> {
        if (!active || route.path !== ownPath) {
            return;
        }
        const id = route.query.h;
        if (typeof id !== 'string' || id.length === 0) {
            return;
        }
        // 等待本轮 DOM 更新后再查找（条件渲染的卡片可能刚显示出来）
        await nextTick();
        if (!active || route.path !== ownPath) {
            return;
        }
        const element = document.querySelector(`[data-setting-id="${CSS.escape(id)}"]`);
        if (element == null) {
            // 找不到元素静默忽略，但仍清掉 h，避免之后每次激活都无效重试
            clearHighlightQuery();
            return;
        }
        element.scrollIntoView({ block: 'center', behavior: 'smooth' });
        element.classList.add('setting-highlight-flash');
        setTimeout(() => element.classList.remove('setting-highlight-flash'), HIGHLIGHT_FLASH_DURATION_MS);
        clearHighlightQuery();
    }

    onMounted(() => {
        active = true;
        void tryHighlight();
    });
    onActivated(() => {
        active = true;
        void tryHighlight();
    });
    onDeactivated(() => {
        active = false;
    });
    watch(() => route.query.h, () => void tryHighlight());
}
