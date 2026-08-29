<script setup lang="ts">
import { onBeforeUnmount, PropType, ref } from 'vue';

import TokenItem from "./TokenItem.vue";
import * as utils from '../logics/utils';

const props = defineProps({
    tokens: {
        type: Object as PropType<{
            token: string;
            marked: boolean;
        }[]>,
        required: true,
    }
});

const emit = defineEmits<{
    /** 拖刷手势开始：父组件应抑制逐词触发的搜索 */
    'paint-start': [];
    /** 拖刷手势结束：父组件应按最终标记状态统一提交一次搜索 */
    'paint-end': [];
}>();

/** 拖刷手势进行中 */
const painting = ref(false);
/** 拖刷的刷子值：手势起点词元切换后的状态，扫过的单词词元一律置为该值（向后扫为标记、向回扫为取消） */
const paintValue = ref(false);
/** 最近一次普通按下的词元索引，作为 Shift+按下范围标记的锚点 */
let lastPressIndex = -1;

function isWordIndex(index: number): boolean {
    return utils.string.isWord(props.tokens[index].token);
}

/** 将 [from, to] 范围内的单词词元置为 value（标点与空白不可标记，跳过） */
function markRange(from: number, to: number, value: boolean): void {
    const [start, end] = from <= to ? [from, to] : [to, from];
    for (let i = start; i <= end; i++) {
        if (isWordIndex(i)) {
            props.tokens[i].marked = value;
        }
    }
}

function addWindowListeners(): void {
    window.addEventListener('mouseup', onWindowMouseup);
    window.addEventListener('blur', onWindowBlur);
}

function removeWindowListeners(): void {
    window.removeEventListener('mouseup', onWindowMouseup);
    window.removeEventListener('blur', onWindowBlur);
}

function endPainting(): void {
    if (!painting.value) {
        return;
    }
    painting.value = false;
    removeWindowListeners();
    emit('paint-end');
}

function onWindowMouseup(): void {
    endPainting();
}

function onWindowBlur(): void {
    // 拖刷中切走窗口导致 mouseup 丢失时收尾，避免手势卡在抑制搜索的状态
    endPainting();
}

/**
 * 在词元上按下鼠标：
 * - 普通按下 = 切换该词元并以切换后的状态为刷子值，进入拖刷手势（未离开首词元时松开即等价单击）；
 * - Shift+按下 = 以最近按下的词元为锚点，将锚点与当前词元之间的单词词元统一置为当前词元切换后的状态。
 */
function onPress(index: number, event: MouseEvent): void {
    if (event.button !== 0 || event.ctrlKey || !isWordIndex(index)) {
        return;
    }
    event.preventDefault();
    if (painting.value) {
        // 上一个手势未正常结束（如在窗口外释放）时先收尾
        endPainting();
    }
    if (event.shiftKey) {
        const value = !props.tokens[index].marked;
        if (lastPressIndex >= 0 && lastPressIndex !== index) {
            markRange(lastPressIndex, index, value);
        } else {
            props.tokens[index].marked = value;
        }
        lastPressIndex = index;
        return;
    }
    props.tokens[index].marked = !props.tokens[index].marked;
    paintValue.value = props.tokens[index].marked;
    lastPressIndex = index;
    painting.value = true;
    addWindowListeners();
    emit('paint-start');
}

/** 拖刷经过词元：将扫过的单词词元置为刷子值（标点与空白跳过，不打断手势） */
function onEnter(index: number): void {
    if (painting.value && isWordIndex(index)) {
        props.tokens[index].marked = paintValue.value;
    }
}

onBeforeUnmount(removeWindowListeners);
</script>

<template>
    <div class="sentence-panel">
        <TokenItem v-for="(token, index) in tokens" :key="index" :token="token.token" v-model:marked="token.marked"
            @press="onPress(index, $event)" @enter="onEnter(index)" />
    </div>
</template>

<style scoped>
.sentence-panel {
    width: 100%;
    height: 100%;
    padding: 8px 12px;
    user-select: none;
    cursor: default;
    background-color: var(--input-text-background-focus);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
    overflow-y: auto;
    overflow-wrap: break-word;
}
</style>
