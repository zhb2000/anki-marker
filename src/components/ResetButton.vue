<script setup lang="ts">
import { HoverWrapper } from '../fluent-controls/HoverWrapper';
</script>

<template>
    <HoverWrapper>
        <button title="重置" class="reset-button">
            <slot></slot>
        </button>
    </HoverWrapper>
</template>

<style scoped>
.reset-button {
    height: 20px;
    width: 20px;
    margin: 0;
    margin-left: 4px;
    padding: 0px;
    border: none;
    border-radius: 4px;
    background-color: var(--window-background);
}

/* 使用 mask-image 显示图标，颜色由 background-color 提供，暗色模式下跟随文本颜色 */
.reset-button::after {
    content: '';
    display: block;
    width: 100%;
    height: 100%;
    background-color: var(--control-text-color);
    mask-image: url('../assets/reset.svg');
    mask-repeat: no-repeat;
    mask-position: center 60%;
    mask-size: 70% 70%;
}

.reset-button[fluent-hovered] {
    filter: var(--icon-button-filter-hover);
}

.reset-button:active {
    filter: var(--icon-button-filter-active);
}
</style>
