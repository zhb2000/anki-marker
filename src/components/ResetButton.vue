<script setup lang="ts">
import { HoverWrapper } from '../fluent-controls/HoverWrapper';
</script>

<template>
    <HoverWrapper>
        <button title="重置" class="reset-button">
            <slot></slot>
        </button>
    </HoverWrapper>
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

.reset-button[fluent-hovered] {
    filter: brightness(90%);
}

.reset-button:active {
    filter: brightness(80%);
}
</style>
