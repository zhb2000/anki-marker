<script setup lang="ts">
import { computed } from 'vue';

import { CONFIG_DEFAULTS, TEXT_SETTING_FALLBACKS, type ConfigModel } from '../logics/config';
import { useSettingsStore } from '../logics/settings-store';
import { HoverWrapper } from '../fluent-controls/HoverWrapper';

/**
 * 单项设置的重置按钮：仅在所在设置项偏离默认值时显示（出现本身即"已修改"指示），
 * tooltip 中展示将要恢复的默认值。与设置仓库耦合，仅用于设置页。
 */
const props = withDefaults(defineProps<{
    /** 对应的设置项键（CONFIG_DEFAULTS 中的键名） */
    settingKey: keyof ConfigModel;
    /** 禁用：所在卡片的功能未启用时置灰并禁止点击 */
    disabled?: boolean;
}>(), {
    disabled: false,
});

const store = useSettingsStore();

/** 当前值等于默认值时不渲染（无意义的 no-op 按钮是噪音） */
const visible = computed(() => store.isModified(props.settingKey));

/** tooltip 文案：留空语义的文本项说明清空后的实际生效值，其余展示默认值本身 */
const tooltip = computed(() => {
    const key = props.settingKey;
    if (key === 'ankiConnectURL' || key === 'deckName' || key === 'modelName') {
        return `清空并使用默认值：${TEXT_SETTING_FALLBACKS[key]}`;
    }
    const value = CONFIG_DEFAULTS[key];
    if (typeof value === 'boolean') {
        return `重置为默认值：${value ? '开启' : '关闭'}`;
    }
    return `重置为默认值：${value === '' ? '留空' : value}`;
});
</script>

<template>
    <Transition name="reset-fade">
        <HoverWrapper v-if="visible">
            <button :title="tooltip" class="reset-button" :disabled="disabled"
                @click="store.reset(settingKey)">
                <slot></slot>
            </button>
        </HoverWrapper>
    </Transition>
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
    /* 静止时弱化图标（辅助操作不与标题争夺注意力），hover/点击时加深 */
    opacity: 0.7;
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

/* 出现/消失过渡：修改值时淡入、恢复默认时淡出，避免按钮突然弹出/抽走 */
.reset-fade-enter-active,
.reset-fade-leave-active {
    transition: opacity 0.1s ease;
}

.reset-fade-enter-from,
.reset-fade-leave-to {
    opacity: 0;
}
</style>
