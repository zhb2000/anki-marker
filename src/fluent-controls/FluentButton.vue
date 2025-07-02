<script setup lang="ts">
import { HoverWrapper } from './HoverWrapper';

const props = defineProps({
    accent: {
        type: Boolean,
        default: false,
    },
});

const emit = defineEmits<{
    'click': [payload: MouseEvent];
}>();
</script>

<template>
    <HoverWrapper>
        <button class="fluent-button" :class="{ accent: props.accent }" @click="emit('click', $event)">
            <!-- 插槽内容允许父组件定制按钮文本或内容 -->
            <slot></slot>
        </button>
    </HoverWrapper>
</template>

<style scoped>
.fluent-button {
    font-family: var(--font-family);
    font-size: var(--font-size);
    color: var(--control-text-color);
    background-color: var(--control-background);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
    user-select: none;
}

.fluent-button[fluent-hovered] {
    background-color: var(--control-background-hover);
}

.fluent-button:active {
    background-color: var(--control-background-active);
    border-bottom-color: var(--border-color);
    color: var(--control-text-color-active);
}

.fluent-button:disabled {
    background-color: var(--control-background-disabled);
    border-bottom-color: var(--border-color);
    color: var(--control-text-color-disabled);
}

.fluent-button.accent {
    color: var(--control-accent-text-color);
    background-color: var(--accent);
    border-color: var(--border-accent-color);
    border-bottom-color: var(--border-accent-bottom-color);
}

.fluent-button.accent[fluent-hovered] {
    background-color: var(--control-accent-background-hover);
}

.fluent-button.accent:active {
    background-color: var(--control-accent-background-active);
    border-bottom-color: var(--border-accent-color);
    color: var(--control-accent-text-color-active);
}

.fluent-button.accent:disabled {
    background-color: var(--control-accent-background-disabled);
    border-bottom-color: var(--border-accent-color);
    color: var(--control-accent-text-color-disabled);
}
</style>
