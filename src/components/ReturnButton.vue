<script setup lang="ts">
import { HoverWrapper } from '../fluent-controls/HoverWrapper';
</script>

<template>
    <HoverWrapper>
        <button class="return-button" title="返回">
            <slot></slot>
        </button>
    </HoverWrapper>
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

.return-button[fluent-hovered] {
    filter: brightness(90%);
}

.return-button:active {
    filter: brightness(80%);
}
</style>
