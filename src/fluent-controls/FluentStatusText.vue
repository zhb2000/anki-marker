<script setup lang="ts">
import { computed } from 'vue';

import infoIcon from '../assets/status-icons/info.svg?url';
import successIcon from '../assets/status-icons/success.svg?url';
import warningIcon from '../assets/status-icons/warning.svg?url';
import errorIcon from '../assets/status-icons/error.svg?url';
import type { StatusSeverity } from './status';

/**
 * 就地反馈文字（图标 + 一行说明，可带不确定进度条）。
 *
 * 提示信息不再占用页面顶部的一条通栏，而是长在触发它的控件旁边（按钮下方、卡片内部、
 * 设置卡片底部），因此组件自身负责“不惊扰用户”：
 * - collapse（默认）：独占一行时，用 grid-template-rows 0fr→1fr 让容器平滑长高/收回，
 *   高度与透明度同时变化，不出现瞬间跳动；文案为空时元素仍在 DOM 里，只是高度为 0。
 * - collapse=false：嵌在横排里时只做淡入淡出，行高完全不变。
 *
 * 图标与配色沿用 WinUI 的 SystemFillColor 语义（与 ContentDialog 的 severity 图标同源）。
 * 间距通过 --status-gap 传入（默认 6px），间距也在被折叠的区域内，收起时不留空档。
 */

const props = withDefaults(defineProps<{
    severity: StatusSeverity;
    /** 文案；为空时收起（触发退场动画） */
    message?: string | null;
    /** 文案下方的不确定进度条，用于进行中的状态 */
    progress?: boolean;
    /** 是否联动高度：独占一行时用默认值，嵌在横排里时传 false */
    collapse?: boolean;
    /** 文案后的操作按钮文字（如「重试」）；为空时不显示 */
    actionLabel?: string | null;
}>(), {
    message: null,
    progress: false,
    collapse: true,
    actionLabel: null,
});

defineEmits<{
    action: [];
}>();

const icons: Record<StatusSeverity, string> = {
    informational: infoIcon,
    success: successIcon,
    warning: warningIcon,
    error: errorIcon,
};

/**
 * 图标走 mask-image（与项目里 ReturnButton / SettingButton 等图标按钮同一做法）：
 * 遮罩图会缩放到元素尺寸，颜色由 background-color 提供，不依赖 SVG 自身的 viewBox
 * 是否被构建工具保留下来（SVGO 会删掉 viewBox，直接当组件用再缩小就会被裁掉）。
 */
const iconStyle = computed(() => ({ '--status-icon': `url("${icons[props.severity]}")` }));
</script>

<template>
    <div class="status" :class="[severity, { 'is-open': message != null, collapsible: collapse }]"
        :role="severity === 'error' ? 'alert' : 'status'"
        :aria-live="severity === 'error' ? 'assertive' : 'polite'">
        <div class="status-inner">
            <div class="status-text">
                <span class="status-icon" :style="iconStyle" aria-hidden="true"></span>
                <div class="status-body">
                    <div class="status-line">
                        <span class="status-message">{{ message }}</span>
                        <button v-if="actionLabel != null" type="button" class="status-action"
                            @click="$emit('action')">{{ actionLabel }}</button>
                    </div>
                    <span v-if="progress" class="status-progress" aria-hidden="true">
                        <span class="status-progress-bar"></span>
                    </span>
                </div>
            </div>
        </div>
    </div>
</template>

<style scoped>
/* 折叠容器：高度由 grid 行轨道驱动，0fr↔1fr 可插值，无需测量像素 */
.status {
    display: grid;
    grid-template-rows: 0fr;
    opacity: 0;
    transform: translateY(-4px);
    transition:
        grid-template-rows var(--motion-duration-enter) var(--motion-ease-standard),
        opacity var(--motion-duration-enter) var(--motion-ease-standard),
        transform var(--motion-duration-enter) var(--motion-ease-standard);
}

.status.is-open {
    grid-template-rows: 1fr;
    opacity: 1;
    transform: none;
}

/* 收起比展开更快，减少“拖泥带水”的余韵 */
.status:not(.is-open) {
    transition-duration: var(--motion-duration-exit);
}

/* 横排内使用：不联动高度，只淡入淡出 */
.status:not(.collapsible) {
    grid-template-rows: 1fr;
}

.status-inner {
    overflow: hidden;
    min-height: 0;
}

/* 间距放在被裁剪的内容里：收起时随内容一起被裁掉，不会留下空档 */
.status-text {
    padding-top: var(--status-gap, 6px);
    display: flex;
    align-items: flex-start;
    gap: 6px;
    font-size: 12px;
    line-height: 16px;
    overflow-wrap: anywhere;
    /* 状态文字是界面反馈而非内容：不可被光标选中，也不显示文本光标
       （所在容器可能是可选中文本的卡片） */
    user-select: none;
    cursor: default;
}

.status-icon {
    flex-shrink: 0;
    width: 12px;
    height: 12px;
    margin-top: 2px;
    background-color: var(--status-icon-color, currentColor);
    mask-image: var(--status-icon);
    mask-repeat: no-repeat;
    mask-position: center center;
    mask-size: 100% 100%;
}

.status-body {
    flex: 1;
    min-width: 0;
}

/* 文案与操作同一行，空间不足时操作折到下一行 */
.status-line {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 8px;
}

/* 操作按钮是「小号文字按钮」而不是超链接：无下划线、无手型光标，
   hover / 按下靠一层很淡的填充提示可点（WinUI 轻量按钮的做法）。
   颜色继承所在提示的 severity 色，避免红字提示里突然出现蓝色链接。 */
.status-action {
    flex-shrink: 0;
    margin-block: -1px;
    padding: 1px 7px;
    border: none;
    border-radius: 4px;
    background-color: color-mix(in srgb, currentColor 6%, transparent);
    color: inherit;
    font: inherit;
    line-height: inherit;
    cursor: default;
    /* 文字与按钮的基线对齐（.status-line 是 baseline 对齐） */
    vertical-align: baseline;
    transition: background-color var(--motion-duration-exit) var(--motion-ease-standard);
}

.status-action:hover {
    background-color: color-mix(in srgb, currentColor 16%, transparent);
}

.status-action:active {
    background-color: color-mix(in srgb, currentColor 24%, transparent);
}

.status-action:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: 1px;
}

/* 文案可换行（如 Anki 版本兼容性提示） */
.status-message {
    white-space: pre-line;
}

/* 图标取 WinUI SystemFillColor 语义，文字用可读性更好的同族颜色 */
.status.informational {
    color: var(--control-text-color);
    --status-icon-color: var(--system-fill-attention);
}

.status.success {
    color: var(--success-text-color);
    --status-icon-color: var(--success-text-color);
}

.status.error {
    color: var(--critical-fill-color);
    --status-icon-color: var(--critical-fill-color);
}

.status.warning {
    color: var(--warning-text-color);
    --status-icon-color: var(--system-fill-caution);
}

.status-progress {
    display: block;
    height: 2px;
    max-width: 180px;
    margin-top: 8px;
    border-radius: 1px;
    overflow: hidden;
    background-color: color-mix(in srgb, currentColor 22%, transparent);
}

.status-progress-bar {
    display: block;
    width: 30%;
    height: 100%;
    border-radius: 1px;
    background-color: var(--accent);
    animation: status-progress 1.4s ease-in-out infinite;
}

@media (prefers-reduced-motion: reduce) {
    .status {
        transition: none;
    }
}

@keyframes status-progress {
    from {
        transform: translateX(-100%);
    }

    to {
        transform: translateX(340%);
    }
}
</style>
