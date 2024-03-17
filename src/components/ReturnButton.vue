<script setup lang="ts">
import { useHover } from '../fluent-controls';

const hover = useHover();
</script>

<template>
    <button class="return-button" :class="hover.classes.value" title="返回" v-bind="$attrs"
        v-on="{ ...hover.listeners, ...$attrs }">
        <slot></slot>
    </button>
</template>

<style scoped>
.return-button {
    height: 32px;
    width: 32px;
    margin: 0;
    padding: 0;
    border: none;
    border-radius: 50%;
    background-image: url('../assets/arrow-back.svg');
    /* background-size: cover; */
    background-repeat: no-repeat;
    background-position: center center;
    background-size: 80% 80%;
    background-color: var(--window-background);
}

.return-button.hover {
    filter: brightness(90%);
}

.return-button:active {
    filter: brightness(80%);
}
</style>
