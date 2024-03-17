<script setup lang="ts">
import { useHover } from '../fluent-controls';

const hover = useHover();
</script>

<template>
    <button title="重置" class="reset-button" :class="hover.classes.value" v-bind="$attrs"
        v-on="{ ...hover.listeners, ...$attrs }">
        <slot></slot>
    </button>
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
    background-image: url('../assets/reset.svg');
    /* background-size: cover; */
    background-repeat: no-repeat;
    background-position: center 60%;
    background-size: 70% 70%;
    background-color: var(--window-background);
}

.reset-button.hover {
    filter: brightness(90%);
}

.reset-button:active {
    filter: brightness(80%);
}
</style>
