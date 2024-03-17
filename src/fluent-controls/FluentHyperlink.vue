<script setup lang="ts">
import { useHover } from './useHover';

const hover = useHover();
</script>

<template>
    <a class="fluent-hyperlink" :class="hover.classes.value" v-bind="$attrs" v-on="{ ...hover.listeners, ...$attrs }">
        <slot></slot>
    </a>
</template>

<style scoped>
.fluent-hyperlink {
    color: var(--accent);
    text-decoration: none;
    border: 0 solid;
    border-radius: var(--border-radius);
}

.fluent-hyperlink:visited {
    color: var(--accent);
}

.fluent-hyperlink.hover {
    color: var(--control-accent-background-hover);
    background-color: rgb(0, 0, 0, 0.1);
}

.fluent-hyperlink:active {
    color: var(--accent);
    background-color: rgb(0, 0, 0, 0.1);
}
</style>
