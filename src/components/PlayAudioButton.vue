<script setup lang="ts">
import { computed, ref, PropType } from 'vue';
import { HoverWrapper } from '../fluent-controls/HoverWrapper';
import { makePronunciationURL } from '../logics/dict';

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

/** 发音失败时就地提示由所在行展示（消息跟随触发它的按钮，不另起全局浮层） */
const emit = defineEmits<{
    error: [message: string];
}>();

const audio = new Audio();

/**
 * 按钮状态：idle 空闲；loading 已点击、正在探测可用发音地址/缓冲；playing 正在出声。
 * loading 与 playing 都显示声波动画并视为“进行中”，此时再点击即停止。
 */
const status = ref<'idle' | 'loading' | 'playing'>('idle');
const active = computed(() => status.value !== 'idle');
const title = computed(() => active.value ? '点击停止发音' : '点击以发音');

/**
 * 每次开始/停止都递增。异步步骤（探测地址、play()）返回后比对令牌，
 * 不一致说明期间已被停止或重新发起，丢弃旧结果，避免连点时重复请求、叠播。
 */
let playToken = 0;

function stop() {
    playToken++;
    audio.pause();
    status.value = 'idle';
}

async function handleClick() {
    if (active.value) {
        stop();
        return;
    }
    const word = props.word.trim();
    if (word.length === 0) {
        return;
    }
    const token = ++playToken;
    status.value = 'loading';
    const url = (await makePronunciationURL(word, props.type))?.url;
    if (token !== playToken) return;
    if (url == null) {
        status.value = 'idle';
        emit('error', '无法获取在线发音');
        return;
    }
    audio.src = url;
    try {
        await audio.play();
    } catch {
        // 被 stop() 中断时 play() 以 AbortError 拒绝，令牌已变，静默丢弃
        if (token !== playToken) return;
        status.value = 'idle';
        emit('error', '发音播放失败');
    }
}

// 监听 audio 事件同步状态
audio.addEventListener('playing', () => {
    status.value = 'playing';
});
audio.addEventListener('ended', () => {
    status.value = 'idle';
});
audio.addEventListener('pause', () => {
    status.value = 'idle';
});
// 播放途中出错（开始前的失败由 play() 的拒绝统一处理，这里不重复提示）
audio.addEventListener('error', () => {
    if (status.value !== 'playing') return;
    status.value = 'idle';
    emit('error', '发音播放失败');
});
</script>

<template>
    <HoverWrapper>
        <button :title="title" :aria-label="title" class="play-audio-button" :class="{ active }" @click="handleClick">
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

/* 使用 mask-image 显示图标，颜色由 background-color 提供，暗色模式下跟随文本颜色。
   Fluent System Icons 20_regular 按 20px 渲染（图形约 16px、线宽 1px）：
   伪元素比 16px 按钮四周各外扩 2px，图形恰好落在按钮范围内。
   play-audio.svg 是横向三帧精灵图（0/1/2 道声波），mask-size 300% 每次只露一帧，
   mask-position 0% / 50% / 100% 分别对应第 0/1/2 帧；静止时显示满声波帧 */
.play-audio-button::after {
    content: '';
    display: block;
    width: 20px;
    height: 20px;
    margin: -2px;
    background-color: var(--control-text-color);
    mask-image: url('../assets/play-audio.svg');
    mask-repeat: no-repeat;
    mask-position: 100% 0;
    mask-size: 300% 100%;
}

/* 进行中（加载 + 播放）：声波 0→1→2 逐帧循环 */
.play-audio-button.active::after {
    animation: speaker-waves 900ms steps(1, end) infinite;
}

@keyframes speaker-waves {
    0% {
        mask-position: 0% 0;
    }

    33.333% {
        mask-position: 50% 0;
    }

    66.667% {
        mask-position: 100% 0;
    }
}

/* 减少动态效果：不播放动画，改用强调色静态指示进行中 */
@media (prefers-reduced-motion: reduce) {
    .play-audio-button.active::after {
        animation: none;
        background-color: var(--accent);
    }
}

.play-audio-button[fluent-hovered] {
    filter: var(--icon-button-filter-hover);
}

.play-audio-button:active {
    filter: var(--icon-button-filter-active);
}
</style>
