<script setup lang="ts">
import { nextTick, watch } from 'vue';
import FluentDialogShell from './FluentDialogShell.vue';
import type { ContentDialogHostState } from './ContentDialog';
import ErrorIcon from '../assets/dialog-icons/error.svg?component';
import WarningIcon from '../assets/dialog-icons/warning.svg?component';

/**
 * 命令式 ContentDialog 的单例宿主（见 ContentDialog.ts）。
 * severity 图标（若有）展示在标题左侧，正文为纯文字；命令区按钮由请求携带。
 *
 * 图标取自 microsoft/fluentui-system-icons（MIT）：
 * error → DismissCircle20Regular，warning → Warning20Regular。
 */

const props = defineProps<{
    /** 由 ContentDialog.ts 创建并传入的共享响应式状态 */
    host: ContentDialogHostState;
}>();

const emit = defineEmits<{
    close: [];
    command: [key: string];
}>();

// 打开时聚焦默认（accent）按钮，对齐 WinUI ContentDialog 的 DefaultButton 焦点行为，
// 使 Enter 直接触发确认、Tab 可在按钮间移动
watch(() => props.host.open, open => {
    if (open) {
        void nextTick(() => {
            document.querySelector<HTMLElement>('.fluent-dialog .command-button.accent')?.focus();
        });
    }
});
</script>

<template>
    <FluentDialogShell :open="host.open" :title="host.request?.title ?? ''"
        :commands="host.request?.commands ?? []"
        @close="emit('close')" @command="emit('command', $event)">
        <template v-if="host.request?.kind === 'error'" #title-icon>
            <ErrorIcon class="severity-icon severity-error" aria-hidden="true" />
        </template>
        <template v-else-if="host.request?.kind === 'warning'" #title-icon>
            <WarningIcon class="severity-icon severity-warning" aria-hidden="true" />
        </template>
        <!-- white-space: pre-line 保留调用方 message 中的换行（如划词失败的权限指引） -->
        <span v-if="host.request != null" class="message-text">{{ host.request.message }}</span>
    </FluentDialogShell>
</template>

<style scoped>
/* severity 图标与 20px 标题文字等高同行；颜色取 fluent-styles.css 中
   按 WinUI token 定义的语义色变量（明暗主题各自映射） */
.severity-icon {
    flex-shrink: 0;
    width: 20px;
    height: 20px;
}

.severity-icon.severity-error {
    color: var(--critical-fill-color);
}

.severity-icon.severity-warning {
    color: var(--warning-text-color);
}

.message-text {
    white-space: pre-line;
    /* 长错误信息可换行滚动，与外壳的内容区滚动行为衔接 */
    overflow-wrap: anywhere;
}
</style>
