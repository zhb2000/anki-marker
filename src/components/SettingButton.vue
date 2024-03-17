<script setup lang="ts">
import { useHover } from '../fluent-controls';

const hover = useHover();
</script>

<template>
    <button class="setting-button" :class="hover.classes.value" title="设置" v-bind="$attrs"
        v-on="{ ...hover.listeners, ...$attrs }">
        <slot></slot>
    </button>
</template>

<style scoped>
.setting-button {
    height: 32px;
    width: 32px;
    margin: 0;
    padding: 0;
    background-image: url('../assets/settings.svg');
    /* background-size: cover; */
    background-repeat: no-repeat;
    background-position: center center;
    background-color: var(--window-background);
    background-size: 60% 60%;
    border: none;
    border-radius: var(--border-radius);
}

.setting-button.hover {
    filter: brightness(90%);
}

.setting-button:active {
    filter: brightness(80%);
}
</style>
