<script setup lang="ts">
import { HoverWrapper } from './HoverWrapper';

const model = defineModel();
</script>

<template>
    <HoverWrapper>
        <select class="fluent-select" v-model="model">
            <slot></slot>
        </select>
    </HoverWrapper>
</template>

<style scoped>
.fluent-select {
    padding-left: 8px;
    padding-right: 8px;
    font-family: var(--font-family);
    font-size: var(--font-size);
    background-color: var(--control-background);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
    user-select: none;
}

.fluent-select[fluent-hovered] {
    background-color: var(--control-background-hover);
}

.fluent-select:active {
    background-color: var(--control-background-active);
    border-bottom-color: var(--border-color);
    color: var(--control-text-color-active);
}
</style>
