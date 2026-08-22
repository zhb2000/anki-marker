<script setup lang="ts">
import { HoverWrapper } from '../fluent-controls/HoverWrapper';
</script>

<template>
    <HoverWrapper>
        <button class="return-button" title="返回">
            <slot></slot>
        </button>
    </HoverWrapper>
</template>

<style scoped>
.return-button {
    height: 32px;
    width: 32px;
    margin: 0;
    padding: 0;
    border: none;
    border-radius: 50%;
    background-color: var(--window-background);
}

/* 使用 mask-image 显示图标，颜色由 background-color 提供，暗色模式下跟随文本颜色 */
.return-button::after {
    content: '';
    display: block;
    width: 100%;
    height: 100%;
    background-color: var(--control-text-color);
    mask-image: url('../assets/arrow-back.svg');
    mask-repeat: no-repeat;
    mask-position: center center;
    mask-size: 80% 80%;
}

.return-button[fluent-hovered] {
    filter: var(--icon-button-filter-hover);
}

.return-button:active {
    filter: var(--icon-button-filter-active);
}
</style>
