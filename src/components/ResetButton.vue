<script setup lang="ts">
import { HoverWrapper } from '../fluent-controls/HoverWrapper';

withDefaults(defineProps<{
    /** 禁用：所在卡片的功能未启用时置灰并禁止点击 */
    disabled?: boolean;
}>(), {
    disabled: false,
});
</script>

<template>
    <HoverWrapper>
        <button title="重置" class="reset-button" :disabled="disabled">
            <slot></slot>
        </button>
    </HoverWrapper>
</template>

<style scoped>
.reset-button {
    height: 20px;
    width: 20px;
    margin: 0;
    margin-left: 4px;
    padding: 0px;
    border: none;
    border-radius: 4px;
    /* 背景透明，不在卡片上留下色块；hover 时才显示底色 */
    background-color: transparent;
}

/* 使用 mask-image 显示图标，颜色由 background-color 提供，暗色模式下跟随文本颜色 */
.reset-button::after {
    content: '';
    display: block;
    width: 100%;
    height: 100%;
    background-color: var(--control-text-color);
    mask-image: url('../assets/reset.svg');
    mask-repeat: no-repeat;
    mask-position: center 60%;
    mask-size: 70% 70%;
    /* 平时弱化图标，hover 时加深 */
    opacity: 0.5;
}

.reset-button[fluent-hovered] {
    background-color: var(--control-background-hover);
}

.reset-button[fluent-hovered]::after {
    opacity: 1;
}

.reset-button:active {
    filter: var(--icon-button-filter-active);
}

/* 禁用态：放在 [fluent-hovered] 规则之后，同优先级时以源序覆盖 hover 底色与图标加深；
   原生 disabled 按钮本身不响应点击，无需脚本拦截 */
.reset-button:disabled {
    background-color: transparent;
}

.reset-button:disabled::after {
    opacity: 0.3;
}
</style>
