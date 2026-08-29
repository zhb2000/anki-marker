<script setup lang="ts">
import { computed } from 'vue';

import { HoverWrapper } from '../fluent-controls/HoverWrapper';
import * as utils from '../logics/utils';

const props = defineProps({
    token: {
        type: String,
        required: true,
    }
});

const emit = defineEmits<{
    /** 在词元上按下鼠标（含非单词词元，由父组件过滤） */
    press: [event: MouseEvent];
    /** 鼠标进入词元（拖刷手势经过时由父组件处理） */
    enter: [event: MouseEvent];
}>();

const marked = defineModel<boolean>('marked', { required: true });

const isWord = computed(() => utils.string.isWord(props.token));

const title = computed(() => {
    if (isWord.value) {
        return marked.value ? '已标记，点击取消' : '点击标记，按住拖动可连续标记';
    } else {
        return undefined;
    }
});
</script>

<template>
    <HoverWrapper>
        <span class="token" :class="{
            marked: marked,
            'is-word': isWord,
            'not-word': !isWord
        }" :title="title" @mousedown="emit('press', $event)" @mouseenter="emit('enter', $event)">{{ token }}</span>
    </HoverWrapper>
</template>

<style scoped>
.token {
    white-space: pre-wrap;
    overflow-wrap: break-word;
    font-size: var(--font-size);
    font-family: var(--font-family);
    padding-left: 2px;
    padding-right: 2px;
    border-radius: 2px;
    user-select: none;
    cursor: default;
    transition: background-color 0.2s, color 0.2s, font-weight 0.2s;
}

/* 单词词元可点击（按下标记或拖刷连标） */
.token.is-word {
    cursor: pointer;
}

.token.is-word.marked {
    background-color: var(--accent);
    color: var(--control-accent-text-color);
    font-weight: bold;
}

.token.is-word[fluent-hovered] {
    outline-style: solid;
    outline-width: 1px;
    outline-color: var(--accent);
    will-change: background-color, color, font-weight;
}

/* 不是单词的空白字符 */
.token.not-word {
    padding-left: 0;
    padding-right: 0;
}
</style>
