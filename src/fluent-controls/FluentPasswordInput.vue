<script setup lang="ts">
import { computed, ref, useAttrs, watch } from 'vue';
import { HoverWrapper } from './HoverWrapper';

// 关闭 attr 自动继承：class/style 手动绑到容器（继承外部的尺寸设置），
// 其余 attrs（placeholder、@blur 等）透传给内部 input
defineOptions({ inheritAttrs: false });

const props = withDefaults(defineProps<{
    /** 眼睛按钮的揭示方式：toggle 点击切换明文/掩码；peek 按住时临时显示明文（对齐 WinUI PasswordBox 的 PasswordRevealMode） */
    revealMode?: 'toggle' | 'peek';
}>(), {
    revealMode: 'toggle',
});

const [model, modifiers] = defineModel<string>({
    set(value) {
        if (value == null) {
            return value;
        }
        let stringValue = String(value);
        if (modifiers.trim) {
            stringValue = stringValue.trim();
        }
        return stringValue;
    }
});

const attrs = useAttrs();

/** 除 class/style 外的透传属性（placeholder、@blur 等）；每次渲染时调用以读取最新 attrs */
function restAttrs() {
    const { class: _class, style: _style, ...rest } = attrs;
    return rest;
}

/** 当前是否明文显示 */
const revealed = ref(false);

const inputType = computed(() => revealed.value ? 'text' : 'password');

/** 内容非空时才显示眼睛按钮（对齐 WinUI PasswordBox 的行为） */
const revealButtonVisible = computed(() => (model.value ?? '').length > 0);

/** 内容被清空时恢复掩码，避免按钮隐藏后新输入的内容仍是明文 */
watch(revealButtonVisible, visible => {
    if (!visible) {
        revealed.value = false;
    }
});

/** toggle 模式：点击切换明文/掩码 */
function handleRevealClick() {
    if (props.revealMode === 'toggle') {
        revealed.value = !revealed.value;
    }
}

/** peek 模式：按住期间临时显示明文；捕获指针以便在按钮外松开也能恢复掩码 */
function handlePeekStart(event: PointerEvent) {
    if (props.revealMode !== 'peek') {
        return;
    }
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    revealed.value = true;
}

/** peek 模式：松开（或指针取消）时恢复掩码 */
function handlePeekEnd() {
    if (props.revealMode !== 'peek') {
        return;
    }
    revealed.value = false;
}
</script>

<template>
    <HoverWrapper>
        <div class="fluent-password-input" :class="$attrs.class" :style="$attrs.style">
            <input v-model="model" class="inner-input" spellcheck="false" autocomplete="off" v-bind="restAttrs()"
                :type="inputType" />
            <!-- @mousedown.prevent 阻止按钮抢夺输入框焦点，避免触发外部绑定的 blur 保存逻辑 -->
            <button v-show="revealButtonVisible" type="button" class="reveal-button" :class="{ revealed }"
                :title="revealed ? '隐藏' : '显示'" :aria-label="revealed ? '隐藏密码' : '显示密码'"
                @mousedown.prevent @click="handleRevealClick" @pointerdown="handlePeekStart"
                @pointerup="handlePeekEnd" @pointercancel="handlePeekEnd"></button>
        </div>
    </HoverWrapper>
</template>

<style scoped>
.fluent-password-input {
    display: flex;
    align-items: center;
    box-sizing: border-box;
    background-color: var(--control-background);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
}

.fluent-password-input[fluent-hovered] {
    background-color: var(--control-background-hover);
}

.fluent-password-input:focus-within {
    background-color: var(--input-text-background-focus);
    border-bottom-width: var(--input-text-border-bottom-width-focus);
    border-bottom-color: var(--accent);
}

.inner-input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding-left: 12px;
    /* 右侧贴近眼睛按钮，留少量间隙 */
    padding-right: 4px;
    border: none;
    outline: none;
    background: transparent;
    line-height: 100%;
    font-family: var(--font-family);
    font-size: var(--font-size);
}

.inner-input::placeholder {
    color: var(--placeholder-color);
}

.inner-input:focus::placeholder {
    color: var(--placeholder-color-focus);
}

.reveal-button {
    flex-shrink: 0;
    width: 30px;
    height: 24px;
    margin-right: 4px;
    padding: 0;
    border: none;
    border-radius: 4px;
    background: transparent;
    cursor: pointer;
}

/* 悬停时给出轻微底色（复用超链接悬停的语义色） */
.reveal-button:hover {
    background-color: var(--hyperlink-hover-background);
}

/* 使用 mask-image 显示图标，颜色由 background-color 提供，暗色模式下跟随文本颜色 */
.reveal-button::after {
    content: '';
    display: block;
    width: 16px;
    height: 16px;
    margin: auto;
    background-color: var(--control-text-color);
    mask-image: url('../assets/eye.svg');
    mask-repeat: no-repeat;
    mask-position: center;
    mask-size: contain;
}

.reveal-button.revealed::after {
    mask-image: url('../assets/eye-off.svg');
}
</style>
