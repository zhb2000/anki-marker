<script setup lang="ts">
import { computed, ref, PropType } from 'vue';
import { HoverWrapper } from '../fluent-controls/HoverWrapper';
import { makePronunciationURL } from '../logics/dict';
import { ElMessage } from 'element-plus';

const props = defineProps({
    word: {
        type: String,
        required: true
    },
    type: {
        type: String as PropType<'en' | 'us'>,
        required: true
    }
});

const audio = new Audio();
const isPlaying = ref(false);
const title = computed(() => isPlaying.value ? '点击停止发音' : '点击以发音');

async function handleClick() {
    const playing = isPlaying.value;
    audio.pause();
    isPlaying.value = false;
    const word = props.word.trim();
    if (word.length === 0) {
        return;
    }
    if (!playing) {
        const url = (await makePronunciationURL(word, props.type))?.url;
        if (url == null) {
            ElMessage.error('无法获取在线发音');
            return;
        }
        audio.src = url;
        await audio.play();
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
    <HoverWrapper>
        <button :title="title" class="play-audio-button" @click="handleClick">
            <slot></slot>
        </button>
    </HoverWrapper>
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

.play-audio-button[fluent-hovered] {
    filter: brightness(90%);
}

.play-audio-button:active {
    filter: brightness(80%);
}
</style>
