<script setup lang="ts">
import { HoverWrapper } from '../fluent-controls/HoverWrapper';
</script>

<template>
    <HoverWrapper>
        <button class="edit-button" title="编辑笔记"></button>
    </HoverWrapper>
</template>

<style scoped>
.edit-button {
    display: block;
    margin-bottom: 8px;
    width: 16px;
    height: 16px;
    padding: 0;
    border: none;
    border-radius: var(--border-radius);
    background-color: var(--icon-button-background);
    /* 使用 mask-image 后，用 background 设置图标的颜色 */
    mask-image: url('../assets/edit.svg');
}

.edit-button[fluent-hovered] {
    background-color: var(--accent);
}

.edit-button:active {
    background-color: var(--control-accent-background-active);
}
</style>
