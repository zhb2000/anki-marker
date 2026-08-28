<script setup lang="ts">
import { computed, ref, useAttrs } from 'vue';
import { HoverWrapper } from './HoverWrapper';

// 关闭 attr 自动继承：clearable 时根节点为容器 div，class/style 需手动绑到容器（继承外部的尺寸设置），
// 其余 attrs（placeholder、type、事件监听等）透传给内部 input；
// 非 clearable 时全部 attrs 仍落在 input 上，DOM 与既有行为完全一致
defineOptions({ inheritAttrs: false });

const props = withDefaults(defineProps<{
    /** 是否启用清空按钮：有文字时在输入框内部右侧显示 ×，点击清空 v-model 并保持输入框焦点 */
    clearable?: boolean;
    /** 禁用输入：禁止编辑并弱化显示；清空按钮同时隐藏（避免禁用状态下仍可清空内容） */
    disabled?: boolean;
}>(), {
    clearable: false,
    disabled: false,
});

const [model, modifiers] = defineModel({
    set(value: unknown) {
        if (value == null) {
            return value;
        }
        // eslint-disable-next-line @typescript-eslint/no-base-to-string
        let stringValue = String(value);
        if (modifiers.trim) {
            stringValue = stringValue.trim();
        }
        return stringValue;
    }
});

const attrs = useAttrs();

/** 内部 input 元素（clearable 结构下用于清空后保持/恢复焦点） */
const inputEl = ref<HTMLInputElement>();

/** 是否显示清空按钮：clearable、未禁用且当前有文字 */
const showClearButton = computed(() => {
    if (!props.clearable || props.disabled || model.value == null) {
        return false;
    }
    // eslint-disable-next-line @typescript-eslint/no-base-to-string
    return String(model.value).length > 0;
});

/** 清空内容（经 v-model 的 set 触发 update:modelValue）并保持输入框焦点 */
function clearInput() {
    model.value = '';
    inputEl.value?.focus();
}

/** 除 class/style 外的透传属性；每次渲染时调用以读取最新 attrs */
function restAttrs() {
    const { class: _class, style: _style, ...rest } = attrs;
    return rest;
}
</script>

<template>
    <!-- 非 clearable：保持既有 DOM 结构不变（HoverWrapper 直接渲染 input，attrs 全部落在 input 上） -->
    <HoverWrapper v-if="!clearable">
        <input v-model="model" class="fluent-input" :disabled="disabled" v-bind="$attrs" />
    </HoverWrapper>
    <!-- clearable：包一层相对定位容器，× 按钮绝对定位在输入框内部右侧；class/style 落在容器上 -->
    <div v-else class="fluent-input-clearable" :class="$attrs.class" :style="$attrs.style">
        <HoverWrapper>
            <input ref="inputEl" v-model="model" class="fluent-input" :disabled="disabled"
                :class="{ 'with-clear-button': showClearButton }" v-bind="restAttrs()" />
        </HoverWrapper>
        <!-- mousedown.prevent 避免点击时夺走输入框焦点；tabindex="-1" 不进入 Tab 序列 -->
        <button v-if="showClearButton" type="button" tabindex="-1" class="clear-button" title="清空"
            @mousedown.prevent @click="clearInput">✕</button>
    </div>
</template>

<style scoped>
.fluent-input {
    padding-left: 12px;
    padding-right: 12px;
    line-height: 100%;
    font-family: var(--font-family);
    font-size: var(--font-size);
    background-color: var(--control-background);
    outline: none;
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
}

.fluent-input[fluent-hovered] {
    background-color: var(--control-background-hover);
}

.fluent-input:focus {
    background-color: var(--input-text-background-focus);
    border-bottom-width: var(--input-text-border-bottom-width-focus);
    border-bottom-color: var(--accent);
}

.fluent-input::placeholder {
    color: var(--placeholder-color);
}

.fluent-input:focus::placeholder {
    color: var(--placeholder-color-focus)
}

/* 禁用态：放在 [fluent-hovered] 之后，同优先级时以后者覆盖 hover 底色（对齐 FluentSelect 的规则顺序） */
.fluent-input:disabled {
    background-color: var(--control-background-disabled);
    border-bottom-color: var(--border-color);
    color: var(--control-text-color-disabled);
}

/* clearable 结构的相对定位容器：尺寸由使用方的 class（如 card-input）设定，input 填满容器 */
.fluent-input-clearable {
    position: relative;
    display: inline-block;
}

.fluent-input-clearable .fluent-input {
    width: 100%;
    height: 100%;
}

/* 有文字且 clearable 时为右侧 × 按钮预留空间，避免遮挡文字 */
.fluent-input-clearable .fluent-input.with-clear-button {
    padding-right: 28px;
}

/* 清空按钮：视觉弱化（平时半透明无底色）；hover 时加深并显示按钮底色，
   配色与 FluentPasswordInput 眼睛按钮的 hover 底色保持一致（--hyperlink-hover-background） */
.clear-button {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    margin: 0;
    padding: 0;
    border: none;
    border-radius: 4px;
    background-color: transparent;
    color: var(--control-text-color);
    font-size: 12px;
    line-height: 1;
    opacity: 0.5;
    cursor: pointer;
}

.clear-button:hover {
    opacity: 1;
    background-color: var(--hyperlink-hover-background);
}
</style>
