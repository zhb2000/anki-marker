<script setup lang="ts">
import { ref, PropType } from 'vue';

import { useHover } from '../fluent-controls';
import AddButton from './AddButton.vue';
import type { CardStatus } from './CardStatus';

const props = defineProps({
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

const boxShadowAnimation = ref<string | undefined>(undefined);

const editButtonHover = useHover();

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
    boxShadowAnimation.value = undefined;
}
</script>

<template>
    <div class="card" :class="{
        'is-added': status === 'is-added' || status === 'processing-add',
        'use-add-box-shadow-animation': boxShadowAnimation === 'use-add-box-shadow-animation',
        'use-remove-box-shadow-animation': boxShadowAnimation === 'use-remove-box-shadow-animation'
    }" @animationend="handleAnimationEnd">
        <div class="display-flex">
            <div class="flex-grow-1">
                <slot></slot>
            </div>
            <div class="flex-shrink-0">
                <AddButton :status="status" class="card-button" @click="emitAddBtnClick" />
                <button class="card-button edit-button" :class="editButtonHover.classes.value"
                    v-on="editButtonHover.listeners" title="编辑笔记" @click="emitEditBtnClick"
                    v-show="status === 'is-added'"></button>
            </div>
        </div>
    </div>
</template>

<style scoped>
.card {
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

.card-button {
    display: block;
    margin-bottom: 8px;
    width: 16px;
    height: 16px;
}

.edit-button {
    padding: 0;
    border: none;
    border-radius: var(--border-radius);
    background-color: #D4D7D7;
    /* 使用 mask-image 后，用 background 设置图标的颜色 */
    mask-image: url('../assets/edit.svg');
}

.edit-button.hover {
    background-color: var(--accent);
}

.edit-button:active {
    background-color: var(--control-accent-background-active);
}
</style>
