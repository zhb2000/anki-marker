<script setup lang="ts">
import { PropType } from 'vue';

import type { CollinsItem } from '../logics/dict';
import type { CardStatus } from './CardStatus';
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
    <WordCard :index="index" :status="status" @add-btn-click="emitAddBtnClick" @edit-btn-click="emitEditBtnClick">
        <b>{{ item.word }}</b>
        <span v-if="item.sense != null"><span v-html="' '"></span><i>{{ item.sense }}</i></span>
        <div v-if="item.enDef != null" v-html="item.enDef"></div>
        <div v-if="item.cnDef != null">{{ item.cnDef }}</div>
    </WordCard>
</template>

<style scoped></style>
