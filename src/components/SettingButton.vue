<script setup lang="ts">
import { computed } from 'vue';
import { ElBadge } from 'element-plus';

import { HoverWrapper } from '../fluent-controls/HoverWrapper';

const props = defineProps({
    updateAvailable: {
        type: Boolean,
        required: true
    }
});

const tooltipTitle = computed(() => props.updateAvailable ? '设置（更新可用）' : '设置');
</script>

<template>
    <ElBadge is-dot :offset="[-2, 2]" :hidden="!props.updateAvailable">
        <HoverWrapper>
            <button class="setting-button" :title="tooltipTitle">
                <slot></slot>
            </button>
        </HoverWrapper>
    </ElBadge>
</template>

<style scoped>
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
