<script setup lang="ts">
import { useHover } from './useHover';

const props = defineProps({
    accent: {
        type: Boolean,
        default: false,
    },
});

const emit = defineEmits<{
    'click': [payload: MouseEvent];
}>();

const hover = useHover();
</script>

<template>
    <button class="fluent-button" :class="{
        ...hover.classes.value,
        accent: props.accent
    }" v-on="hover.listeners" @click="emit('click', $event)">
        <!-- 插槽内容允许父组件定制按钮文本或内容 -->
        <slot></slot>
    </button>
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

.fluent-button.hover {
    background-color: var(--control-background-hover);
}

.fluent-button:active {
    background-color: var(--control-background-active);
    border-bottom-color: var(--border-color);
    color: var(--control-text-color-active);
}

.fluent-button.accent {
    color: var(--control-accent-text-color);
    background-color: var(--accent);
    border-color: var(--border-accent-color);
    border-bottom-color: var(--border-accent-bottom-color);
}

.fluent-button.accent.hover {
    background-color: var(--control-accent-background-hover);
}

.fluent-button.accent:active {
    background-color: var(--control-accent-background-active);
    border-bottom-color: var(--border-accent-color);
    color: var(--control-accent-text-color-active);
}
</style>
