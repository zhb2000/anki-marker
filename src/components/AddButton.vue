<script setup lang="ts">
import { PropType } from 'vue';

import { HoverWrapper } from '../fluent-controls/HoverWrapper';
import type { CardStatus } from './CardStatus';

const props = defineProps({
    /**
     * - `not-added`: 未添加到 Anki
     * - `processing`: 正在处理中
     * - `is-added`: 已添加到 Anki
     */
    status: {
        type: String as PropType<CardStatus>,
        required: true
    }
});

const statusToTitle = {
    'not-added': '添加到 Anki',
    'processing-add': '处理中',
    'processing-remove': '处理中',
    'is-added': '已添加，点击可撤销'
};
</script>

<template>
    <HoverWrapper>
        <button :title="statusToTitle[props.status]" class="add-btn" :class="{
            'is-added': props.status === 'is-added',
            'processing': props.status === 'processing-add' || props.status === 'processing-remove'
        }"></button>
    </HoverWrapper>
</template>

<style scoped>
.add-btn {
    border-width: 0;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background-color: #D4D7D7;
    position: relative;
}

.add-btn[fluent-hovered] {
    background-color: var(--control-accent-background-hover);
}

.add-btn:active {
    background-color: var(--control-accent-background-active);
}

/* 加号的横线 */
.add-btn::before {
    content: '';
    width: 8px;
    height: 2px;
    background-color: var(--control-accent-text-color);
    position: absolute;
    top: 7px;
    left: 4px;
}

/* 加号的竖线 */
.add-btn::after {
    content: '';
    width: 2px;
    height: 8px;
    background-color: var(--control-accent-text-color);
    position: absolute;
    top: 4px;
    left: 7px;
}

.add-btn:active::before {
    background-color: var(--control-accent-text-color-active);
}

.add-btn:active::after {
    background-color: var(--control-accent-text-color-active);
}

.add-btn.is-added {
    background-color: var(--accent);
}

.add-btn.is-added[fluent-hovered] {
    background-color: var(--control-accent-background-hover);
}

.add-btn.processing {
    cursor: not-allowed;
    --processing-background-color: #e4e4e4;
    background-color: var(--processing-background-color);
}
</style>
