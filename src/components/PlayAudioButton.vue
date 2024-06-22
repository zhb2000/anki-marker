<script setup lang="ts">
import { useHover } from '../fluent-controls';

const hover = useHover();
</script>

<template>
    <button title="点击以发音" class="play-audio-button" :class="hover.classes.value" v-on="hover.listeners">
        <slot></slot>
    </button>
</template>

<style scoped>
.play-audio-button {
    height: 16px;
    width: 16px;
    margin-right: 8px;
    margin-left: 2px;
    padding: 0;
    background-image: url('../assets/play-audio.svg');
    /* background-size: cover; */
    background-repeat: no-repeat;
    background-position: center center;
    background-color: var(--window-background);
    background-size: 160% 160%;
    border: none;
    border-radius: 2px;
}

.play-audio-button.hover {
    filter: brightness(90%);
}

.play-audio-button:active {
    filter: brightness(80%);
}
</style>
