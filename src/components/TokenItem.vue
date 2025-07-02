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

const marked = defineModel<boolean>('marked', { required: true });

const isWord = computed(() => utils.string.isWord(props.token));

const title = computed(() => {
    if (isWord.value) {
        return marked.value ? '已标记，点击取消' : '点击以标记';
    } else {
        return undefined;
    }
});

function handleClick() {
    // 只有单词才能标记
    if (isWord.value) {
        marked.value = !marked.value;
    }
}
</script>

<template>
    <HoverWrapper>
        <span class="token" :class="{
            marked: marked,
            'is-word': isWord,
            'not-word': !isWord
        }" :title="title" @click="handleClick">{{ token }}</span>
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
    transition: background-color 0.2s, color 0.2s, font-weight 0.2s;
}

.token.is-word.marked {
    background-color: var(--accent);
    color: white;
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
