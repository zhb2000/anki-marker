<script setup lang="ts">
import { ref, PropType } from 'vue';

import AddButton from './AddButton.vue';
import EditButton from './EditButton.vue';
import type { CardStatus } from './CardStatus';
import FluentStatusText from '../fluent-controls/FluentStatusText.vue';
import type { StatusNotice } from '../fluent-controls/status';

const props = defineProps({
    index: {
        type: Number,
        required: true
    },
    status: {
        type: String as PropType<CardStatus>,
        required: true
    },
    /** 该卡片自己的就地反馈（连接进度、编辑降级提示等）；null 时收起 */
    notice: {
        type: Object as PropType<StatusNotice | null>,
        default: null
    }
});

const emit = defineEmits<{
    'add-btn-click': [index: number];
    'edit-btn-click': [index: number];
    /** 就地反馈里的操作（如失败后的「重试」） */
    'notice-action': [index: number];
}>();

const boxShadowAnimation = ref<string | null>(null);

function emitAddBtnClick() {
    if (props.status === 'not-added') {
        boxShadowAnimation.value = 'use-add-box-shadow-animation';
    } else {
        boxShadowAnimation.value = 'use-remove-box-shadow-animation';
    }
    emit('add-btn-click', props.index);
}

function emitEditBtnClick() {
    emit('edit-btn-click', props.index);
}

function handleAnimationEnd() {
    boxShadowAnimation.value = null;
}
</script>

<template>
    <div class="card" :class="{
        'is-added': status === 'is-added' || status === 'processing-add' || status === 'processing-remove',
        'use-add-box-shadow-animation': boxShadowAnimation === 'use-add-box-shadow-animation',
        'use-remove-box-shadow-animation': boxShadowAnimation === 'use-remove-box-shadow-animation'
    }" @animationend="handleAnimationEnd">
        <div class="display-flex">
            <div class="flex-grow-1">
                <slot></slot>
            </div>
            <div class="card-actions">
                <AddButton :status="status" @click="emitAddBtnClick" />
                <EditButton v-show="status === 'is-added'" @click="emitEditBtnClick" />
            </div>
        </div>
        <!-- 反馈挂在卡片内部：与触发它的条目同处一处，且随卡片一起滚动 -->
        <FluentStatusText class="card-notice" :severity="notice?.severity ?? 'informational'"
            :message="notice?.message ?? null" :progress="notice?.progress ?? false"
            :action-label="notice?.actionLabel ?? null" @action="emit('notice-action', index)" />
    </div>
</template>

<style scoped>
.card {
    user-select: text;
    cursor: text;
    padding: 10px;
    margin-bottom: 10px;
    font-family: var(--font-family);
    font-size: var(--font-size);
    color: var(--control-text-color);
    background-color: var(--input-text-background-focus);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
    transition: box-shadow calc(0.3s * 0.5);
}

.card-notice {
    margin-top: 6px;
}

.card.is-added {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
}

.use-add-box-shadow-animation {
    animation: addBoxShadow 0.3s;
}

.use-remove-box-shadow-animation {
    animation: removeBoxShadow 0.3s;
}

@keyframes addBoxShadow {
    0% {
        box-shadow: initial;
    }

    50% {
        box-shadow: inset 0 0 0 3px var(--accent);
    }

    100% {
        box-shadow: inset 0 0 0 1px var(--accent);
    }
}

@keyframes removeBoxShadow {
    0% {
        box-shadow: inset 0 0 0 1px var(--accent);
    }

    50% {
        box-shadow: inset 0 0 0 3px var(--accent);
    }

    100% {
        box-shadow: initial
    }
}

.display-flex {
    display: flex;
    gap: 4px;
}

.flex-grow-1 {
    flex-grow: 1;
}

.flex-shrink-0 {
    flex-shrink: 0;
}

/* 右侧按钮列：两个 24px 命中盒纵向排列，间距 4px */
.card-actions {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: center;
}
</style>
