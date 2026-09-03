<script setup lang="ts">
import { onBeforeUnmount, watch } from 'vue';

/**
 * WinUI ContentDialog 风格的弹窗外壳（遮罩 + 容器 + 标题 + 底部命令条）。
 *
 * 视觉规格：smoke 遮罩（30% 黑）、弹层用 flyout token（8px 圆角、SurfaceStroke 描边、
 * 双层阴影）、底部命令条 44px 高、按钮通栏均分、相邻按钮 1px 分隔、扁平无圆角。
 *
 * 职责边界：外壳只负责框架与生命周期（Teleport、Esc/点遮罩关闭），标题以下的内容
 * （表单、列表、状态等）由使用方通过默认插槽提供；命令条由 commands 数组声明，
 * 点击时以 command 事件回传 key，由使用方决定动作。
 */

/** 底部命令条的单个命令；只有一个命令时通栏显示 */
export interface FluentDialogCommand {
    /** 命令标识，点击时通过 command 事件回传 */
    key: string;
    label: string;
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
    /** 点击命令条按钮，参数为命令 key */
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
        <!-- mousedown.self：仅点击遮罩空白处关闭，弹窗内部的点击不受影响 -->
        <div v-if="open" class="fluent-dialog-backdrop" @mousedown.self="emit('close')">
            <div class="fluent-dialog" role="dialog" aria-modal="true" :aria-label="title">
                <div class="dialog-title">{{ title }}</div>
                <div class="dialog-body">
                    <slot></slot>
                </div>
                <div v-if="commands != null && commands.length > 0" class="dialog-commands"
                    :style="{ gridTemplateColumns: `repeat(${commands.length}, 1fr)` }">
                    <button v-for="(command, index) in commands" :key="command.key" type="button"
                        class="command-button" :class="{ 'command-separator': index > 0 }"
                        :disabled="command.disabled" @click="emit('command', command.key)">
                        {{ command.label }}
                    </button>
                </div>
            </div>
        </div>
    </Teleport>
</template>

<style scoped>
/* 遮罩：WinUI ContentDialog 的 smoke 背板（30% 黑） */
.fluent-dialog-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: grid;
    place-items: center;
    background-color: rgba(0, 0, 0, 0.3);
}

/* 弹层容器：flyout token（8px 圆角/SurfaceStroke 描边/双层阴影），纵向布局；
   超高时整体收缩，内部内容区自行滚动 */
.fluent-dialog {
    display: flex;
    flex-direction: column;
    width: min(480px, calc(100vw - 48px));
    max-height: calc(100vh - 48px);
    overflow: hidden;
    color: var(--control-text-color);
    font-family: var(--font-family);
    background-color: var(--flyout-background);
    border: 1px solid var(--flyout-border-color);
    border-radius: var(--flyout-border-radius);
    box-shadow: var(--flyout-shadow);
}

.dialog-title {
    flex-shrink: 0;
    padding: 20px 24px 12px;
    font-size: 20px;
    font-weight: 600;
    user-select: none;
}

.dialog-body {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    padding: 0 24px;
}

/* 底部命令条：ContentDialog 规格——44px 高、通栏均分、相邻 1px 分隔、扁平无圆角 */
.dialog-commands {
    display: grid;
    flex-shrink: 0;
    height: 44px;
    margin-top: 12px;
    border-top: 1px solid var(--border-color);
}

.command-button {
    border: none;
    background-color: transparent;
    color: var(--control-text-color);
    font-family: var(--font-family);
    font-size: 14px;
    cursor: pointer;
}

.command-button:hover {
    background-color: var(--flyout-item-background-hover);
}

.command-button:active {
    background-color: var(--flyout-item-background-active);
}

.command-button:disabled {
    color: var(--control-text-color-disabled);
    cursor: default;
}

.command-button:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: -2px;
}

.command-separator {
    border-left: 1px solid var(--border-color);
}
</style>
