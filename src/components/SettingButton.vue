<script setup lang="ts">
import { computed } from 'vue';

import { FluentInfoBadge } from '../fluent-controls';
import { HoverWrapper } from '../fluent-controls/HoverWrapper';
import { settingsNeedAttention } from '../logics/settings-attention';

/**
 * 设置入口按钮。
 *
 * 红点聚合设置里所有“需要关注”的项（更新、辅助功能权限、快捷键冲突……），
 * 判定与设置页导航项同源（见 logics/settings-attention.ts）：用户在主页看到红点，
 * 进设置一定能找到对应分区。
 */
const tooltipTitle = computed(() => settingsNeedAttention.value ? '设置（有需要关注的项）' : '设置');
</script>

<template>
    <div class="setting-button-wrapper">
        <HoverWrapper>
            <button class="setting-button" :title="tooltipTitle" :aria-label="tooltipTitle">
                <slot></slot>
            </button>
        </HoverWrapper>
        <FluentInfoBadge v-if="settingsNeedAttention" class="setting-badge" />
    </div>
</template>

<style scoped>
.setting-button-wrapper {
    position: relative;
    display: inline-flex;
}

/* WinUI InfoBadge 贴图标右上角；不接收指针事件，避免挡住按钮点击。
   WinUI 原值 4px 是给导航列表项（旁边有文字）的，落在 32px 图标按钮角上几乎不可见，
   这里按“适度加强可辨度”放大到 6px */
.setting-badge {
    position: absolute;
    top: 2px;
    right: 2px;
    --info-badge-dot-size: 6px;
    pointer-events: none;
}

.setting-button {
    height: 32px;
    width: 32px;
    margin: 0;
    padding: 0;
    background-color: var(--window-background);
    border: none;
    border-radius: var(--border-radius);
}

/* 使用 mask-image 显示图标，颜色由 background-color 提供，暗色模式下跟随文本颜色 */
.setting-button::after {
    content: '';
    display: block;
    width: 100%;
    height: 100%;
    background-color: var(--control-text-color);
    mask-image: url('../assets/settings.svg');
    mask-repeat: no-repeat;
    mask-position: center center;
    mask-size: 60% 60%;
}

.setting-button[fluent-hovered] {
    filter: var(--icon-button-filter-hover);
}

.setting-button:active {
    filter: var(--icon-button-filter-active);
}
</style>
