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
/* 链接文字用 --accent-text 而非 --accent：深色下强调色作正文只有 3.6:1，
   WinUI 同样区分“强调色填充”与“强调色文字”两套 token */
.fluent-hyperlink {
    color: var(--accent-text);
    text-decoration: none;
    border: 0 solid;
    border-radius: var(--border-radius);
    cursor: pointer;
}

.fluent-hyperlink:visited {
    color: var(--accent-text);
}

.fluent-hyperlink[fluent-hovered] {
    color: var(--accent-text);
    background-color: var(--hyperlink-hover-background);
}

.fluent-hyperlink:active {
    color: var(--accent-text);
    background-color: var(--hyperlink-hover-background);
}
</style>
