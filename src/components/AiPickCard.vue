<script setup lang="ts">
import { computed, type PropType } from 'vue';

import type { AiPickState, DictSource } from '../logics/aiPick';
import AddButton from './AddButton.vue';
import type { CardStatus } from './CardStatus';

const props = defineProps({
    state: {
        type: Object as PropType<AiPickState>,
        required: true
    },
    /** 加号按钮状态，跟随 AI 选中的那条词典条目的添加状态 */
    status: {
        type: String as PropType<CardStatus>,
        default: 'not-added'
    }
});

const emit = defineEmits<{
    'add-btn-click': [];
}>();

/** 完成态且命中真实词典条目（非 fallback）时才显示加号，语义等同该条目卡片上的加号 */
const showAddButton = computed(() => props.state.phase === 'done' && props.state.pick != null);

function emitAddBtnClick() {
    emit('add-btn-click');
}

/** 词典来源的显示名 */
const DICT_LABELS: Record<DictSource, string> = {
    collins: '柯林斯',
    oxford: '牛津',
    youdao: '有道'
};

/** 完成态且命中词典条目时的来源名 */
const sourceLabel = computed(() => {
    const pick = props.state.pick;
    return pick != null ? DICT_LABELS[pick.source] : null;
});
</script>

<template>
    <div class="ai-pick-card" :class="{ 'is-fallback': state.phase === 'done' && state.fallback }">
        <!-- 与 WordCard 相同的左右分栏：左侧内容自适应，右侧加号按钮固定 -->
        <div class="display-flex">
            <div class="flex-grow-1">
                <div class="ai-pick-header">
                    <span class="ai-badge">AI</span>
                    <span v-if="state.phase === 'done' && sourceLabel != null" class="ai-source-badge">
                        优选 · {{ sourceLabel }}
                    </span>
                    <span v-else-if="state.phase === 'done' && state.fallback" class="ai-source-badge ai-source-warning">
                        AI 生成，非词典条目
                    </span>
                </div>
                <div v-if="state.phase === 'loading'" class="ai-skeleton" aria-hidden="true">
                    <div class="ai-skeleton-line"></div>
                    <div class="ai-skeleton-line ai-skeleton-line-short"></div>
                </div>
                <!-- key 随 phase 变化：进入 streaming / done 时重新挂载，重放淡入动画 -->
                <div v-else class="ai-pick-body" :key="state.phase">
                    <div class="ai-contextual-def">
                        {{ state.contextualDef }}<span v-if="state.phase === 'streaming'" class="ai-caret"></span>
                    </div>
                    <div v-if="state.note.length > 0" class="ai-note">笔记：{{ state.note }}</div>
                </div>
            </div>
            <div v-if="showAddButton" class="flex-shrink-0">
                <AddButton :status="status" class="card-button" @click="emitAddBtnClick" />
            </div>
        </div>
    </div>
</template>

<style scoped>
.ai-pick-card {
    user-select: text;
    cursor: text;
    padding: 10px;
    /* 卡片位于滚动容器内部，与词典卡片一致只保留下边距 */
    margin-bottom: 10px;
    font-family: var(--font-family);
    font-size: var(--font-size);
    color: var(--control-text-color);
    background-color: var(--input-text-background-focus);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    /* 克制的 AI 区隔：accent 左边条 */
    border-left: 2px solid var(--accent);
    border-radius: var(--border-radius);
    animation: aiPickCardIn calc(0.3s * 0.5) ease-out;
}

.ai-pick-card.is-fallback {
    border-left-color: var(--warning-text-color);
}

/* 以下加号按钮布局样式与 WordCard 一致 */
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

.card-button {
    display: block;
    margin-bottom: 8px;
    width: 16px;
    height: 16px;
}

.ai-pick-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
    user-select: none;
}

.ai-badge {
    padding: 2px 5px;
    font-size: 11px;
    font-weight: 600;
    line-height: 1.4;
    border-radius: 3px;
    color: var(--control-accent-text-color);
    background-color: var(--accent);
}

.ai-source-badge {
    font-size: 12px;
    color: var(--control-text-color-active);
}

.ai-source-warning {
    color: var(--warning-text-color);
}

.ai-pick-body {
    animation: aiPickBodyIn calc(0.3s * 0.5) ease-out;
}

.ai-contextual-def {
    overflow-wrap: break-word;
}

.ai-caret {
    display: inline-block;
    width: 2px;
    height: 1em;
    margin-left: 2px;
    vertical-align: -0.15em;
    background-color: var(--accent);
    animation: aiPickPulse 1s step-end infinite;
}

.ai-note {
    margin-top: 4px;
    font-size: 13px;
    color: var(--control-text-color-active);
}

.ai-skeleton-line {
    height: 1em;
    margin: 4px 0;
    border-radius: 3px;
    background-color: var(--processing-background);
    animation: aiPickPulse 1.2s ease-in-out infinite;
}

.ai-skeleton-line-short {
    width: 60%;
}

@keyframes aiPickCardIn {
    from {
        opacity: 0;
        transform: translateY(-4px);
    }

    to {
        opacity: 1;
        transform: translateY(0);
    }
}

@keyframes aiPickBodyIn {
    from {
        opacity: 0;
        transform: translateY(-2px);
    }

    to {
        opacity: 1;
        transform: translateY(0);
    }
}

@keyframes aiPickPulse {

    0%,
    100% {
        opacity: 0.45;
    }

    50% {
        opacity: 1;
    }
}
</style>
