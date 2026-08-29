<script setup lang="ts">
import { computed, ref } from 'vue';

/**
 * WinUI 3 ToggleSwitch 风格开关。
 * 视觉与交互参数对齐官方模板（ToggleSwitch_themeresources.xaml）：
 * 40x20 轨道（圆角 10）、12px 旋钮、20px 行程；
 * 悬停旋钮放大到 14px，按压/拖动时变为 17x14 的胶囊形（圆角固定 7px）；
 * 支持在旋钮/轨道上按住拖动，松开时越过轨道中点即提交切换（与 WinUI 行为一致）。
 */

/** 指针位移超过该值（px）才视为拖动，否则视为点击 */
const DRAG_THRESHOLD = 5;
/** 旋钮行程：轨道 40 - 左右各 4px 内边距 - 旋钮 12 */
const KNOB_TRAVEL = 20;
/** 拖动提交阈值：旋钮左缘越过轨道中点（行程一半）即提交，对齐 WinUI 实现 */
const COMMIT_MIDPOINT = 10;
/** 按压未拖动时旋钮向轨道中心偏移的量（px），对应 WinUI Pressed 态的 margin 调整 */
const PRESS_SHIFT = 1.5;

const props = defineProps<{
    disabled?: boolean;
}>();

const model = defineModel<boolean>();

const rootEl = ref<HTMLElement | null>(null);

/** 指针已按下（含尚未超过拖动阈值的阶段） */
const pressing = ref(false);
/** 已进入拖动（超过阈值，旋钮跟手） */
const dragging = ref(false);
/** 拖动中旋钮的 translateX（0..KNOB_TRAVEL） */
const dragX = ref(0);

let pointerId = -1;
let pressStartX = 0;

const knobStyle = computed(() => {
    let x = dragging.value ? dragX.value : (model.value ? KNOB_TRAVEL : 0);
    if (pressing.value && !dragging.value) {
        x += model.value ? -PRESS_SHIFT : PRESS_SHIFT;
    }
    return { transform: `translateX(${x}px)` };
});

function toggle() {
    if (!props.disabled) {
        model.value = !model.value;
    }
}

function onPointerDown(e: PointerEvent) {
    if (props.disabled || (e.pointerType === 'mouse' && e.button !== 0)) return;
    pressing.value = true;
    dragging.value = false;
    pointerId = e.pointerId;
    pressStartX = e.clientX;
    dragX.value = model.value ? KNOB_TRAVEL : 0;
    rootEl.value?.setPointerCapture(e.pointerId);
}

function onPointerMove(e: PointerEvent) {
    if (!pressing.value || props.disabled || e.pointerId !== pointerId) return;
    const dx = e.clientX - pressStartX;
    if (!dragging.value && Math.abs(dx) < DRAG_THRESHOLD) return;
    dragging.value = true;
    dragX.value = Math.min(KNOB_TRAVEL, Math.max(0, (model.value ? KNOB_TRAVEL : 0) + dx));
}

function onPointerUp(e: PointerEvent) {
    if (!pressing.value || e.pointerId !== pointerId) return;
    pointerId = -1;
    const wasDragging = dragging.value;
    pressing.value = false;
    dragging.value = false;
    if (!wasDragging) {
        // 未发生拖动：视为点击，直接切换
        toggle();
        return;
    }
    // 拖动提交：按旋钮位置是否越过轨道中点决定是否切换（未越过则原样弹回）
    const shouldOn = dragX.value > COMMIT_MIDPOINT;
    if (shouldOn !== !!model.value) {
        toggle();
    }
}

function onPointerCancel(e: PointerEvent) {
    // pointercancel / 捕获意外丢失：复位并让旋钮弹回，不切换
    if (e.pointerId !== pointerId) return;
    pointerId = -1;
    pressing.value = false;
    dragging.value = false;
}

function onKeydown(e: KeyboardEvent) {
    if (props.disabled || e.repeat) return;
    if (e.key === ' ' || e.key === 'Enter') {
        e.preventDefault();
        toggle();
    }
}
</script>

<template>
    <span ref="rootEl" class="fluent-switch" :class="{ checked: !!model, disabled: props.disabled, pressing, dragging }"
        role="switch" :aria-checked="!!model" :aria-disabled="props.disabled || undefined"
        :tabindex="props.disabled ? -1 : 0" @pointerdown="onPointerDown" @pointermove="onPointerMove"
        @pointerup="onPointerUp" @pointercancel="onPointerCancel" @lostpointercapture="onPointerCancel"
        @keydown="onKeydown">
        <span class="fluent-switch-knob" :style="knobStyle">
            <span class="fluent-switch-knob-circle" />
        </span>
    </span>
</template>

<style scoped>
.fluent-switch {
    position: relative;
    display: inline-block;
    box-sizing: border-box;
    width: 40px;
    height: 20px;
    border-radius: 10px;
    /* off 态: ToggleSwitchFillOff(ControlAltFillColorSecondary) + ToggleSwitchStrokeOff(ControlStrongStrokeColorDefault) */
    background-color: var(--switch-fill-off);
    border: 1px solid var(--switch-stroke-off);
    outline: none;
    user-select: none;
    touch-action: none;
    cursor: default;
    transition: background-color 83ms linear, border-color 83ms linear;
}

.fluent-switch:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: 2px;
}

/* off 态 hover / 按压: ControlAltFillColorTertiary / Quarternary */
.fluent-switch:not(.disabled):hover {
    background-color: var(--switch-fill-off-hover);
}

.fluent-switch:not(.disabled).pressing {
    background-color: var(--switch-fill-off-active);
}

/* on 态: 主题色轨道，描边与填充同色（WinUI 的 On 态描边厚度为 0） */
.fluent-switch.checked {
    background-color: var(--switch-fill-on);
    border-color: var(--switch-fill-on);
}

.fluent-switch.checked:not(.disabled):hover {
    background-color: var(--switch-fill-on-hover);
    border-color: var(--switch-fill-on-hover);
}

.fluent-switch.checked:not(.disabled).pressing {
    background-color: var(--switch-fill-on-active);
    border-color: var(--switch-fill-on-active);
}

/* 禁用态: ControlAltFillColorDisabled / ControlStrongStrokeColorDisabled；
   on 态为 AccentFillColorDisabled */
.fluent-switch.disabled {
    background-color: var(--switch-fill-off-disabled);
    border-color: var(--switch-stroke-off-disabled);
}

.fluent-switch.disabled.checked {
    background-color: var(--switch-fill-on-disabled);
    border-color: var(--switch-stroke-on-disabled);
}

/* 旋钮外壳负责位移（translateX 由内联样式给出），拖动时直接跟手不做补间 */
.fluent-switch-knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 12px;
    height: 12px;
    transition: transform 200ms cubic-bezier(0, 0, 0, 1);
}

.fluent-switch.dragging .fluent-switch-knob {
    transition: none;
}

/* 旋钮本体负责尺寸变化（hover 12->14、按压 17x14）。圆角固定 7px 与官方模板一致：
   12/14px 时为圆，17x14 时因 7 < 17/2 而呈胶囊形。绝对定位居中于旋钮外壳，
   使宽高变化时以中心对称生长（对应官方 HorizontalAlignment=Center 的布局动画） */
.fluent-switch-knob-circle {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 12px;
    height: 12px;
    border-radius: 7px;
    /* off 态: ToggleSwitchKnobFillOff(TextFillColorSecondary) */
    background-color: var(--switch-knob-off);
    transform: translate(-50%, -50%);
    transition: width 83ms cubic-bezier(0, 0, 0, 1), height 83ms cubic-bezier(0, 0, 0, 1),
        background-color 83ms linear;
}

.fluent-switch:not(.disabled):hover .fluent-switch-knob-circle {
    width: 14px;
    height: 14px;
}

.fluent-switch:not(.disabled).pressing .fluent-switch-knob-circle {
    /* 按压/拖动：17x14 胶囊形（WinUI Pressed 态） */
    width: 17px;
    height: 14px;
}

/* on 态旋钮: ToggleSwitchKnobFillOn(TextOnAccentFillColorPrimary)，
   描边为 CircleElevationBorderBrush 的近似纯色 */
.fluent-switch.checked .fluent-switch-knob-circle {
    background-color: var(--switch-knob-on);
    box-shadow: inset 0 0 0 1px var(--switch-knob-ring);
}

.fluent-switch.disabled .fluent-switch-knob-circle {
    /* ToggleSwitchKnobFillOffDisabled(TextFillColorDisabled) */
    background-color: var(--switch-knob-off-disabled);
}

.fluent-switch.disabled.checked .fluent-switch-knob-circle {
    /* ToggleSwitchKnobFillOnDisabled(TextOnAccentFillColorDisabled) */
    background-color: var(--switch-knob-on-disabled);
}

@media (prefers-reduced-motion: reduce) {

    .fluent-switch,
    .fluent-switch-knob,
    .fluent-switch-knob-circle {
        transition-duration: 0.01ms;
    }
}
</style>
