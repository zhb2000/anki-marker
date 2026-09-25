<script setup lang="ts">
import { onBeforeUnmount, watch } from 'vue';
import FluentButton from './FluentButton.vue';

/**
 * WinUI ContentDialog 风格的弹窗外壳（遮罩 + 容器 + 标题 + 底部命令区）。
 *
 * 视觉规格取自 microsoft-ui-xaml 的 ContentDialog_themeresources.xaml（Fluent v2）：
 * smoke 遮罩 SmokeFillColorDefault(#0000004D)；容器 OverlayCornerRadius(8px)、1px
 * SurfaceStrokeColorDefault 描边、尺寸区间 320x184 ~ 548x756；内容区叠加
 * LayerFillColorAlt 底色并以 1px CardStrokeColorDefault 分隔线与命令区分隔；
 * 内边距 24px；标题 20px SemiBold；命令区按钮等宽分列、间距 8px、高 32px，
 * 默认按钮（accent）使用强调色样式；打开/关闭带 1.05 缩放 + 淡入淡出过渡。
 *
 * 职责边界：外壳只负责框架与生命周期（Teleport、Esc/点遮罩关闭），标题以下的内容
 * （表单、列表、状态等）由使用方通过默认插槽提供；命令区由 commands 数组声明，
 * 点击时以 command 事件回传 key，由使用方决定动作。标题左侧可经 title-icon
 * 插槽放置 severity 图标（与 20px 标题文字等高同行）。
 */

/** 底部命令区的单个命令；只有一个命令时占右半（对齐 WinUI CloseButton 行为） */
export interface FluentDialogCommand {
    /** 命令标识，点击时通过 command 事件回传 */
    key: string;
    label: string;
    /** 默认按钮（ContentDialog DefaultButton），使用 Accent 强调样式 */
    accent?: boolean;
    disabled?: boolean;
}

const props = defineProps<{
    /** 弹窗是否可见（挂载/卸载由该值驱动） */
    open: boolean;
    title: string;
    commands?: FluentDialogCommand[];
}>();

const emit = defineEmits<{
    /** Esc 或点击遮罩空白处时触发 */
    close: [];
    /** 点击命令区按钮，参数为命令 key */
    command: [key: string];
}>();

function onKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
        emit('close');
    }
}

watch(() => props.open, open => {
    if (open) {
        window.addEventListener('keydown', onKeydown);
    } else {
        window.removeEventListener('keydown', onKeydown);
    }
});

onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown));
</script>

<template>
    <Teleport to="body">
        <Transition name="fluent-dialog">
            <!-- mousedown.self：仅点击遮罩空白处关闭，弹窗内部的点击不受影响 -->
            <div v-if="open" class="fluent-dialog-backdrop" @mousedown.self="emit('close')">
                <div class="fluent-dialog" role="dialog" aria-modal="true" :aria-label="title">
                    <div class="dialog-content">
                        <div class="dialog-title"><slot name="title-icon"></slot>{{ title }}</div>
                        <div class="dialog-body">
                            <slot></slot>
                        </div>
                    </div>
                    <div v-if="commands != null && commands.length > 0" class="dialog-commands"
                        :class="{ 'single-command': commands.length === 1 }"
                        :style="commands.length > 1 ? { gridTemplateColumns: `repeat(${commands.length}, 1fr)` } : undefined">
                        <FluentButton v-for="command in commands" :key="command.key"
                            class="command-button" :accent="command.accent ?? false"
                            :disabled="command.disabled ?? false"
                            @click="emit('command', command.key)">
                            {{ command.label }}
                        </FluentButton>
                    </div>
                </div>
            </div>
        </Transition>
    </Teleport>
</template>

<style scoped>
/* 遮罩：ContentDialog 的 smoke 背板（SmokeFillColorDefault = #0000004D） */
.fluent-dialog-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: grid;
    place-items: center;
    background-color: var(--dialog-smoke-color);
}

/* 弹层容器：OverlayCornerRadius(8px)、1px SurfaceStroke 描边、投影；
   尺寸区间 320x184 ~ 548x756（视口四周至少留 24px），纵向布局，超高时内容区滚动 */
.fluent-dialog {
    display: flex;
    flex-direction: column;
    width: min(548px, calc(100vw - 48px));
    min-width: min(320px, calc(100vw - 48px));
    min-height: 184px;
    max-height: min(756px, calc(100vh - 48px));
    overflow: hidden;
    color: var(--control-text-color);
    font-family: var(--font-family);
    background-color: var(--dialog-background);
    border: 1px solid var(--dialog-border-color);
    border-radius: var(--flyout-border-radius);
    box-shadow: var(--dialog-shadow);
}

/* 内容区：LayerFillColorAlt 叠加底色 + 底部 1px CardStroke 分隔线，24px 内边距；
   超高时标题与内容一起滚动（对应 ContentScrollViewer） */
.dialog-content {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    padding: 24px;
    overflow-y: auto;
    background-color: var(--dialog-content-background);
    border-bottom: 1px solid var(--dialog-separator-color);
}

.dialog-title {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 12px;
    font-size: 20px;
    font-weight: 600;
    user-select: none;
}

.dialog-body {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    font-size: 14px;
}

/* 底部命令区：ContentDialog 规格——按钮等宽分列、间距 8px（ContentDialogButtonSpacing）、
   24px 内边距 */
.dialog-commands {
    display: grid;
    flex-shrink: 0;
    gap: 8px;
    padding: 24px;
}

/* 单命令场景对齐 WinUI：命令区固定为左右两列（PrimaryColumn 与 CloseColumn 各占 *），
   唯一按钮放右列、占右半宽度，左半留空 */
.dialog-commands.single-command {
    grid-template-columns: 1fr 1fr;
}

.dialog-commands.single-command .command-button {
    grid-column: 2;
}

/* 命令按钮复用 FluentButton（WinUI Button 样式），此处对齐 ContentDialog 规格：
   列内拉伸、高 32px（ContentDialogButtonHeight）、字号 14px（ControlContentThemeFontSize） */
.command-button {
    box-sizing: border-box;
    width: 100%;
    min-height: 32px;
    padding: 4px 11px;
    font-size: 14px;
}

.command-button:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: -2px;
}

/* 打开/关闭过渡：淡入淡出 + 1.05 缩放（ControlFastOutSlowInKeySpline 缓动） */
.fluent-dialog-enter-active,
.fluent-dialog-leave-active {
    transition: opacity 0.167s linear;
}

.fluent-dialog-enter-active .fluent-dialog {
    transition: transform 0.25s cubic-bezier(0.1, 0.9, 0.2, 1), opacity 0.167s linear;
}

.fluent-dialog-leave-active .fluent-dialog {
    transition: transform 0.25s cubic-bezier(0.1, 0.9, 0.2, 1), opacity 0.083s linear;
}

.fluent-dialog-enter-from,
.fluent-dialog-leave-to {
    opacity: 0;
}

.fluent-dialog-enter-from .fluent-dialog,
.fluent-dialog-leave-to .fluent-dialog {
    transform: scale(1.05);
    opacity: 0;
}
</style>
