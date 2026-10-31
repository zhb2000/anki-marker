<script setup lang="ts">
import { PropType } from 'vue';

import type { CollinsItem } from '../logics/dict';
import type { CardStatus } from './CardStatus';
import type { StatusNotice } from '../fluent-controls/status';
import WordCard from './WordCard.vue';

const props = defineProps({
    item: {
        type: Object as PropType<CollinsItem>,
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
    },
    /** 该卡片的就地反馈（由 MainView 按词条设置） */
    notice: {
        type: Object as PropType<StatusNotice | null>,
        default: null
    }
});

const emit = defineEmits<{
    'add-btn-click': [index: number];
    'edit-btn-click': [index: number];
    'notice-action': [index: number];
}>();

function emitAddBtnClick() {
    emit('add-btn-click', props.index);
}

function emitEditBtnClick() {
    emit('edit-btn-click', props.index);
}
</script>

<template>
    <WordCard :index="index" :status="status" :notice="notice" :class="{ 'ai-picked': aiPicked }"
        @add-btn-click="emitAddBtnClick"
        @edit-btn-click="emitEditBtnClick" @notice-action="emit('notice-action', index)">
        <span v-if="aiPicked" class="ai-picked-badge">AI 优选</span>
        <b>{{ item.word }}</b>
        <span v-if="item.sense != null"><span v-html="' '"></span><i>{{ item.sense }}</i></span>
        <div v-if="item.enDef != null" v-html="item.enDef"></div>
        <div v-if="item.cnDef != null">{{ item.cnDef }}</div>
    </WordCard>
</template>

<style scoped>
/* .ai-picked 的左边条与背景 tint 由 WordCard 统一实现（色带画法见那里的注释） */

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
