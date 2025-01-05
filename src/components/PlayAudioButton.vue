<script setup lang="ts">
import { computed, ref } from 'vue';
import { useHover } from '../fluent-controls';

const props = defineProps({
    audioSrc: {
        type: String,
        required: true
    }
});

const hover = useHover();
const audio = new Audio();
const isPlaying = ref(false);
const title = computed(() => isPlaying.value ? '点击停止发音' : '点击以发音');

function playPronunciation() {
    const playing = isPlaying.value;
    audio.pause();
    if (!playing) {
        audio.src = props.audioSrc;
        audio.play();
    }
}

// 监听 audio 的播放和暂停事件，确保状态同步
audio.addEventListener('ended', () => {
    isPlaying.value = false;
});
audio.addEventListener('pause', () => {
    isPlaying.value = false;
});
audio.addEventListener('play', () => {
    isPlaying.value = true;
});
</script>

<template>
    <button :title="title" class="play-audio-button" :class="hover.classes.value" v-on="hover.listeners"
        @click="playPronunciation">
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
