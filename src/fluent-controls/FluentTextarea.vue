<script setup lang="ts">
import { ref, useAttrs } from 'vue';
import { HoverWrapper } from './HoverWrapper';
import { useTextModel } from './useTextModel';

// 根节点是 HoverWrapper（无渲染包装），关闭自动继承后由内部 textarea 显式接收 attrs：
// class/style 与 placeholder、事件监听等都落在 textarea 上，与 FluentInput 的 DOM 语义一致
defineOptions({ inheritAttrs: false });

defineProps<{
    /** 禁用输入：禁止编辑并弱化显示 */
    disabled?: boolean;
}>();

const [model, modifiers] = defineModel<string | null>({
    set(value: string | null | undefined) {
        if (value == null) {
            return value;
        }
        return String(value);
    }
});

// 内部 textarea 绑定草稿而非 model：.trim 只在提交时裁剪，逐键输入不吞空白（见 useTextModel）
const { text, commitTrim } = useTextModel(model, modifiers);

const attrs = useAttrs();

/** 内部 textarea 元素 */
const textareaEl = ref<HTMLTextAreaElement>();

/** 供父组件聚焦（与原生 textarea.focus 同名同语义，避免父组件直接触摸内部元素） */
function focus(): void {
    textareaEl.value?.focus();
}

defineExpose({ focus });
</script>

<template>
    <HoverWrapper>
        <textarea ref="textareaEl" v-model="text" class="fluent-textarea" :disabled="disabled"
            @change="commitTrim" v-bind="attrs" />
    </HoverWrapper>
</template>

<style scoped>
/* 与 FluentInput 共用同一套 WinUI 文本框外观：底色、描边、圆角与聚焦下划线。
   尺寸（宽高）由使用方的 class 决定，与 FluentInput 一致 */
.fluent-textarea {
    padding: 8px 12px;
    font-family: var(--font-family);
    font-size: var(--font-size);
    background-color: var(--control-background);
    outline: none;
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
    overflow-wrap: break-word;
    /* WinUI 的多行文本框没有右下角的尺寸拖拽手柄 */
    resize: none;
}

.fluent-textarea[fluent-hovered] {
    background-color: var(--control-background-hover);
}

/* 聚焦下划线：边框宽度始终保持 --border-width（不挤压内容区，文字零位移），
   仅把底边框设透明，accent 色带（background）透过它显示并向上多覆盖厚度差值，
   形成上缘平直、下缘随圆角弯曲的强调下划线（总厚度 --input-text-border-bottom-width-focus）。
   与 FluentInput 的实现一致，保证两种输入控件的聚焦态完全同形 */
.fluent-textarea:focus {
    background-color: var(--input-text-background-focus);
    border-bottom-color: transparent;
    background-image: linear-gradient(var(--accent), var(--accent));
    background-origin: border-box;
    background-size: 100% var(--input-text-border-bottom-width-focus);
    background-position: 0 100%;
    background-repeat: no-repeat;
}

.fluent-textarea::placeholder {
    color: var(--placeholder-color);
}

.fluent-textarea:focus::placeholder {
    color: var(--placeholder-color-focus);
}

/* 禁用态：放在 [fluent-hovered] 之后，同优先级时以后者覆盖 hover 底色（对齐 FluentInput 的规则顺序） */
.fluent-textarea:disabled {
    background-color: var(--control-background-disabled);
    border-bottom-color: var(--border-color);
    color: var(--control-text-color-disabled);
}
</style>
