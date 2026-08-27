<script setup lang="ts">
import { PropType } from 'vue';

import type { YoudaoItem } from '../logics/dict';
import type { CardStatus } from './CardStatus';
import WordCard from './WordCard.vue';

const props = defineProps({
    item: {
        type: Object as PropType<YoudaoItem>,
        required: true
    },
    index: {
        type: Number,
        required: true
    },
    status: {
        type: String as PropType<CardStatus>,
        required: true
    },
    /** 是否为 AI 优选命中的条目（高亮卡片并显示徽章） */
    aiPicked: {
        type: Boolean,
        default: false
    }
});

const emit = defineEmits<{
    'add-btn-click': [index: number];
    'edit-btn-click': [index: number];
}>();

function emitAddBtnClick() {
    emit('add-btn-click', props.index);
}

function emitEditBtnClick() {
    emit('edit-btn-click', props.index);
}
</script>

<template>
    <WordCard :index="index" :status="status" :class="{ 'ai-picked': aiPicked }" @add-btn-click="emitAddBtnClick"
        @edit-btn-click="emitEditBtnClick">
        <span v-if="aiPicked" class="ai-picked-badge">AI 优选</span>
        <b>{{ item.word }}</b>
        <span v-if="item.sense != null"><span v-html="' '"></span><i>{{ item.sense }}</i></span>
        <div v-if="item.cnDef != null">{{ item.cnDef }}</div>
    </WordCard>
</template>

<style scoped>
.ai-picked {
    border-left: 2px solid var(--accent);
    /* color-mix 不可用的旧 WebView 回退为原卡片背景色，仅保留左边条 */
    background-color: var(--input-text-background-focus);
    background-color: color-mix(in srgb, var(--accent) 6%, var(--input-text-background-focus));
}

.ai-picked-badge {
    float: right;
    margin-left: 6px;
    padding: 1px 4px;
    font-size: 11px;
    font-weight: 600;
    line-height: 1.4;
    border-radius: 3px;
    user-select: none;
    color: var(--control-accent-text-color);
    background-color: var(--accent);
}
</style>
