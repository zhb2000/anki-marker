<script setup lang="ts">
import { computed } from 'vue';

import redoIcon from '../assets/icons/redo.svg?url';
import undoIcon from '../assets/icons/undo.svg?url';
import { HoverWrapper } from '../fluent-controls/HoverWrapper';

const props = defineProps<{
    icon: 'undo' | 'redo';
    /** 无障碍名称与悬停提示（图标按钮没有可见文字） */
    label: string;
    disabled?: boolean;
}>();

const emit = defineEmits<{ click: [] }>();

const maskImage = computed(() => `url("${props.icon === 'undo' ? undoIcon : redoIcon}")`);
</script>

<template>
    <HoverWrapper>
        <button class="history-button" type="button" :title="label" :aria-label="label"
            :disabled="disabled" @click="emit('click')">
            <span class="history-glyph" aria-hidden="true" :style="{ maskImage, WebkitMaskImage: maskImage }" />
        </button>
    </HoverWrapper>
</template>

<style scoped>
/* 与 header 里的文字按钮同高同底：一排控件不该有两种体格 */
.history-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    margin: 0;
    padding: 0;
    color: var(--control-text-color);
    background-color: var(--control-background);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
}

.history-button[fluent-hovered]:not(:disabled) {
    background-color: var(--control-background-hover);
}

.history-button:active:not(:disabled) {
    background-color: var(--control-background-active);
    border-bottom-color: var(--border-color);
}

.history-button:disabled {
    /* 只压暗图标，不用禁用底色：header 里两格灰块比两个淡图标更抢眼 */
    color: var(--control-text-color-disabled);
}

/* 用 mask + currentColor 上色：图标跟随主题，不必为深色模式再出一套文件 */
.history-glyph {
    display: block;
    width: 20px;
    height: 20px;
    background-color: currentColor;
    mask-repeat: no-repeat;
    mask-position: center center;
    mask-size: 20px 20px;
}
</style>
