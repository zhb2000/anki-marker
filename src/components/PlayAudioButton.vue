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
    background-color: var(--window-background);
    border: none;
    border-radius: 2px;
}

/* 使用 mask-image 显示图标，颜色由 background-color 提供，暗色模式下跟随文本颜色 */
.play-audio-button::after {
    content: '';
    display: block;
    width: 100%;
    height: 100%;
    background-color: var(--control-text-color);
    mask-image: url('../assets/play-audio.svg');
    mask-repeat: no-repeat;
    mask-position: center center;
    mask-size: 160% 160%;
}

.play-audio-button[fluent-hovered] {
    filter: var(--icon-button-filter-hover);
}

.play-audio-button:active {
    filter: var(--icon-button-filter-active);
}
</style>
