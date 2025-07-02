<script setup lang="ts">
import { HoverWrapper } from './HoverWrapper';

const [model, modifiers] = defineModel({
    set(value: unknown) {
        if (value == null) {
            return value;
        }
        // eslint-disable-next-line @typescript-eslint/no-base-to-string
        let stringValue = String(value);
        if (modifiers.trim) {
            stringValue = stringValue.trim();
        }
        return stringValue;
    }
});
</script>

<template>
    <HoverWrapper>
        <input v-model="model" class="fluent-input" />
    </HoverWrapper>
</template>

<style scoped>
.fluent-input {
    padding-left: 12px;
    padding-right: 12px;
    line-height: 100%;
    font-family: var(--font-family);
    font-size: var(--font-size);
    background-color: var(--control-background);
    outline: none;
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
}

.fluent-input[fluent-hovered] {
    background-color: var(--control-background-hover);
}

.fluent-input:focus {
    background-color: var(--input-text-background-focus);
    border-bottom-width: var(--input-text-border-bottom-width-focus);
    border-bottom-color: var(--accent);
}

.fluent-input::placeholder {
    color: var(--placeholder-color);
}

.fluent-input:focus::placeholder {
    color: var(--placeholder-color-focus)
}
</style>
