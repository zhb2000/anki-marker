<script setup lang="ts">
import { HoverWrapper } from './HoverWrapper';
</script>

<template>
    <HoverWrapper>
        <a class="fluent-hyperlink">
            <slot></slot>
        </a>
    </HoverWrapper>
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

.fluent-hyperlink[fluent-hovered] {
    color: var(--control-accent-background-hover);
    background-color: var(--hyperlink-hover-background);
}

.fluent-hyperlink:active {
    color: var(--accent);
    background-color: var(--hyperlink-hover-background);
}
</style>
