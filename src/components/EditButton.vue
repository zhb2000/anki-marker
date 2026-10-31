<script setup lang="ts">
import { HoverWrapper } from '../fluent-controls/HoverWrapper';

/**
 * “编辑笔记”图标按钮。与 AddButton 共用同一套命中盒规范：
 * 24×24 的盒子承担 hover 面（微填充），内部 16px 图标颜色不变。
 */
</script>

<template>
    <HoverWrapper>
        <button type="button" class="edit-button" title="编辑笔记" aria-label="编辑笔记">
            <span class="edit-icon" aria-hidden="true"></span>
        </button>
    </HoverWrapper>
</template>

<style scoped>
.edit-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    margin: 0;
    padding: 0;
    border: none;
    border-radius: 4px;
    background-color: transparent;
    transition: background-color var(--motion-duration-exit) var(--motion-ease-standard);
}

.edit-button[fluent-hovered] {
    background-color: var(--nav-item-background-hover);
}

.edit-button:active {
    background-color: var(--nav-item-background-pressed);
}

.edit-button:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: -2px;
}

/* 图标用 mask 绘制，颜色由 background-color 提供，暗色模式跟随文本色 */
.edit-icon {
    width: 16px;
    height: 16px;
    background-color: var(--control-text-color);
    mask-image: url('../assets/edit.svg');
    mask-repeat: no-repeat;
    mask-position: center center;
    mask-size: contain;
}
</style>
