<script setup lang="ts">
import { useHover } from './useHover';
import { generateUniqueId } from './generateUniqueId';

const props = defineProps<{ label?: string; }>();

const model = defineModel();

const radioHover = useHover();
const labelHover = useHover();

const radioId = generateUniqueId('fluent-radio');
</script>

<template>
    <span class="fluent-radio-container">
        <input :id="radioId" type="radio" class="fluent-radio"
            :class="{ hover: radioHover.hovered.value || labelHover.hovered.value }" v-model="model"
            v-on="radioHover.listeners" />
        <label v-if="props.label != null" :for="radioId" class="fluent-radio-label" v-on="labelHover.listeners">{{
            props.label }}</label>
    </span>
</template>

<style scoped>
.fluent-radio-container {
    display: flex;
    align-items: center;
}

.fluent-radio-label {
    margin-left: 4px;
    user-select: none;
}

/* https://xiaotianxia.github.io/blog/vuepress/css/styled_switch.html */
.fluent-radio {
    appearance: none;
    position: relative;
    display: inline-block;
    vertical-align: middle;
    padding: 0;
    width: 16px;
    height: 16px;
    border: 1px solid var(--radio-border-color);
    outline: none;
    transition: all 0.2s ease;
    border-radius: 50%;
    background-color: var(--radio-background);
}

.fluent-radio.hover {
    background-color: var(--radio-background-hover);
}

.fluent-radio:active {
    background-color: var(--radio-background-active);
}

.fluent-radio:checked {
    background-color: var(--accent);
    border-width: 0;
}

.fluent-radio:checked.hover {
    background-color: var(--control-accent-background-hover);
}

.fluent-radio:checked:active {
    background-color: var(--control-accent-background-active);
}

.fluent-radio:after {
    content: '';
    display: inline-block;
    position: absolute;
    left: 50%;
    top: 50%;
    background: var(--control-accent-text-color);
    border-radius: 50%;
    width: 0;
    height: 0;
    opacity: 0;
    transform: translate(-50%, -50%);
    transition: all .2s ease;
    transform-origin: center center;
    pointer-events: none;
}

/* .fluent-radio:active::after {
    width: calc(16px - 8px);
    height: calc(16px - 8px);
    opacity: 1;
} */

.fluent-radio:checked:after {
    width: calc(16px - 8px);
    height: calc(16px - 8px);
    opacity: 1;
}

.fluent-radio:checked.hover:after {
    width: calc(16px - 6px);
    height: calc(16px - 6px);
}

.fluent-radio:checked:active:after {
    width: calc(16px - 8px);
    height: calc(16px - 8px);
}
</style>
