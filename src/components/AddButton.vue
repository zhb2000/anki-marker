<script setup lang="ts">
import { computed, PropType } from 'vue';

import { HoverWrapper } from '../fluent-controls/HoverWrapper';
import type { CardStatus } from './CardStatus';

/**
 * 词条的「添加到 Anki / 已添加」切换按钮。
 *
 * 交互与视觉约定：
 * - 24×24 的命中盒同时是 hover 面（WinUI 图标按钮做法）：hover/按下只微填充盒底，
 *   图标本身与所在圆形不变色，避免把普通图标按钮渲染成强调按钮；
 * - 三态图形交叉淡入淡出：未添加「+」、处理中「进度环」、已添加「✓」，状态不靠颜色也能辨认；
 * - 处理中不可点击（aria-disabled，忽略点击），不再用 not-allowed 光标。
 */
const props = defineProps({
    status: {
        type: String as PropType<CardStatus>,
        required: true
    }
});

const emit = defineEmits<{
    click: [];
}>();

const isProcessing = computed(() => props.status === 'processing-add' || props.status === 'processing-remove');

const label = computed(() => {
    switch (props.status) {
        case 'processing-add':
            return '正在添加到 Anki';
        case 'processing-remove':
            return '正在从 Anki 移除';
        case 'is-added':
            return '已添加到 Anki，点击可移除';
        default:
            return '添加到 Anki';
    }
});

function handleClick(): void {
    if (isProcessing.value) {
        return;
    }
    emit('click');
}
</script>

<template>
    <HoverWrapper>
        <button type="button" class="add-btn" :class="[status, { processing: isProcessing }]" :title="label"
            :aria-label="label" :aria-busy="isProcessing" :aria-disabled="isProcessing" @click="handleClick">
            <span class="glyph glyph-plus" aria-hidden="true"></span>
            <span class="glyph glyph-ring" aria-hidden="true"></span>
            <span class="glyph glyph-check" aria-hidden="true"></span>
        </button>
    </HoverWrapper>
</template>

<style scoped>
/* 命中盒：同时承担 hover 面，图标本体只有 16px */
.add-btn {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    margin: 0;
    padding: 0;
    border: none;
    border-radius: 4px;
    background-color: transparent;
    transition: background-color var(--motion-duration-exit) var(--motion-ease-standard);
}

.add-btn[fluent-hovered]:not(.processing) {
    background-color: var(--nav-item-background-hover);
}

.add-btn:active:not(.processing) {
    background-color: var(--nav-item-background-pressed);
}

.add-btn:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: -2px;
}

/* 圆底：16px，随状态换底色（未添加灰、处理中更暗、已添加强调色） */
.add-btn::before {
    content: '';
    position: absolute;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background-color: var(--icon-button-background);
    transition: background-color var(--motion-duration-enter) var(--motion-ease-standard);
}

.add-btn.processing::before {
    background-color: var(--processing-background);
}

.add-btn.is-added::before {
    background-color: var(--accent);
}

/* 三态图形：交叉淡入 + 轻微缩放，切换时不跳变 */
.glyph {
    position: absolute;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    opacity: 0;
    transform: scale(0.8);
    transition:
        opacity var(--motion-duration-enter) var(--motion-ease-standard),
        transform var(--motion-duration-enter) var(--motion-ease-standard);
}

/* 「+」：两条 2px 细棒 */
.glyph-plus::before,
.glyph-plus::after {
    content: '';
    position: absolute;
    border-radius: 1px;
    background-color: var(--control-text-color);
}

.glyph-plus::before {
    width: 8px;
    height: 2px;
}

.glyph-plus::after {
    width: 2px;
    height: 8px;
}

/* 「✓」：用两条边框拼出对勾，颜色取强调底上的文字色 */
.glyph-check::before {
    content: '';
    width: 4px;
    height: 8px;
    margin-top: -2px;
    border-right: 2px solid var(--control-accent-text-color);
    border-bottom: 2px solid var(--control-accent-text-color);
    border-radius: 1px;
    transform: rotate(42deg);
}

/* 进度环：中性轨道 + 强调色弧段，旋转表示进行中 */
.glyph-ring::before {
    content: '';
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2px solid color-mix(in srgb, var(--control-text-color) 18%, transparent);
    border-top-color: var(--accent);
    animation: add-btn-spin 900ms linear infinite;
}

/* 当前状态对应的图形淡入 */
.add-btn.not-added .glyph-plus,
.add-btn.processing .glyph-ring,
.add-btn.is-added .glyph-check {
    opacity: 1;
    transform: scale(1);
}

/* 已添加是强调色控件，对勾弹入带一点回弹 */
.add-btn.is-added .glyph-check {
    transition-timing-function: cubic-bezier(0.2, 1.4, 0.4, 1);
    transition-duration: 180ms;
}

@keyframes add-btn-spin {
    to {
        transform: rotate(360deg);
    }
}

@media (prefers-reduced-motion: reduce) {
    .add-btn,
    .glyph,
    .add-btn::before {
        transition: none;
    }

    .glyph-ring::before {
        animation: none;
        border-top-color: var(--accent);
        border-right-color: var(--accent);
    }
}
</style>
