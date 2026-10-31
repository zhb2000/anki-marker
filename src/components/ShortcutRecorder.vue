<script setup lang="ts">
import { ref, computed, watch, onBeforeUnmount, onDeactivated } from 'vue';

import { formatShortcut } from '../logics/shortcut';
import { invoke } from '../logics/utils';

const props = defineProps<{ modelValue: string }>();
const emit = defineEmits<{ 'update:modelValue': [value: string] }>();

/** 是否处于录制状态 */
const recording = ref(false);
/** 组件根元素 */
const rootElement = ref<HTMLButtonElement | null>(null);

/** 修饰键的 KeyboardEvent.code 集合：仍按住修饰键时等待用户继续 */
const MODIFIER_CODES = new Set([
    'ShiftLeft', 'ShiftRight', 'ControlLeft', 'ControlRight',
    'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight',
    'Fn', 'FnLock', 'CapsLock'
]);

/** 将键盘事件转换为全局快捷键字符串（如 "Cmd+Shift+KeyS"）；组合不完整时返回 null */
function eventToShortcut(event: KeyboardEvent): string | null {
    // metaKey 统一记为 "Cmd"：macOS 上为 Cmd 键，Windows/Linux 上为 Win/Super 键——
    // 后端 global-hotkey 把 Cmd/Meta/Super 解析为同一修饰键，跨平台无需区分；
    // 显示层（logics/shortcut.ts）按平台映射为对应名称
    const modifiers: string[] = [];
    if (event.metaKey) modifiers.push('Cmd');
    if (event.ctrlKey) modifiers.push('Ctrl');
    if (event.altKey) modifiers.push('Alt');
    if (event.shiftKey) modifiers.push('Shift');
    // 至少需要一个非 Shift 的修饰键，避免快捷键与正常打字冲突
    if (!event.metaKey && !event.ctrlKey && !event.altKey) {
        return null;
    }
    if (MODIFIER_CODES.has(event.code)) {
        return null;
    }
    return [...modifiers, event.code].join('+');
}

/**
 * 录制期间的键盘事件处理（挂载在 window 捕获阶段）。
 *
 * 不能将 keydown 绑定在按钮上：macOS 的 WKWebView（同 Safari）中点击 button
 * 不会使其获得焦点，按钮上的 keydown 收不到任何事件。
 */
function handleRecordingKeydown(event: KeyboardEvent) {
    event.preventDefault();
    event.stopPropagation();
    if (event.code === 'Escape') {
        recording.value = false; // 取消录制
        return;
    }
    if (event.code === 'Backspace' || event.code === 'Delete') {
        recording.value = false;
        if (props.modelValue.length > 0) {
            emit('update:modelValue', ''); // 清除快捷键
        }
        return;
    }
    const shortcut = eventToShortcut(event);
    if (shortcut == null) {
        return;
    }
    recording.value = false;
    if (shortcut !== props.modelValue) {
        emit('update:modelValue', shortcut);
    }
}

/** 录制期间点击组件外的区域时取消录制，避免录制状态拦截其他控件的键盘输入 */
function handleRecordingMousedown(event: MouseEvent) {
    if (event.target instanceof Node && rootElement.value?.contains(event.target) !== true) {
        recording.value = false;
    }
}

/** 窗口失焦（切到别的应用）时取消录制：录制期间全局快捷键是暂停的，不能一直停着 */
function handleWindowBlur() {
    recording.value = false;
}

/**
 * 录制期间暂停全局快捷键：系统级热键会先把按键吃掉，录制控件收不到；同时还会触发取词、
 * 跳回主界面。结束时按配置文件重新注册（幂等）——保存新快捷键时 commit_config 也会按新配置
 * 重新注册，两者谁先谁后，最终注册的都是配置文件里的值
 */
function setGlobalShortcutSuspended(suspended: boolean) {
    invoke(suspended ? 'suspend_global_shortcut' : 'resume_global_shortcut').catch((error: unknown) => {
        console.error(error);
    });
}

watch(recording, recording => {
    if (recording) {
        window.addEventListener('keydown', handleRecordingKeydown, true);
        window.addEventListener('mousedown', handleRecordingMousedown, true);
        window.addEventListener('blur', handleWindowBlur);
    } else {
        window.removeEventListener('keydown', handleRecordingKeydown, true);
        window.removeEventListener('mousedown', handleRecordingMousedown, true);
        window.removeEventListener('blur', handleWindowBlur);
    }
    setGlobalShortcutSuspended(recording);
});

// 所在页面被 KeepAlive 停用时结束录制（watch 随之恢复全局快捷键）
onDeactivated(() => {
    recording.value = false;
});

onBeforeUnmount(() => {
    window.removeEventListener('keydown', handleRecordingKeydown, true);
    window.removeEventListener('mousedown', handleRecordingMousedown, true);
    window.removeEventListener('blur', handleWindowBlur);
    // 卸载时组件的 watch 已停止，不会再触发恢复，这里补上
    if (recording.value) {
        setGlobalShortcutSuspended(false);
    }
});

const display = computed(() => {
    if (recording.value) {
        return '请按下组合键（Esc 取消，退格键清除）';
    }
    if (props.modelValue.length === 0) {
        return '点击设置快捷键';
    }
    return formatShortcut(props.modelValue);
});
</script>

<template>
    <button type="button" ref="rootElement" class="shortcut-recorder" :class="{ recording }"
        :title="modelValue.length > 0 ? '点击后按下新的组合键可更改，按退格键清除' : undefined"
        @click="recording = !recording">
        {{ display }}
    </button>
</template>

<style scoped>
.shortcut-recorder {
    padding-left: 12px;
    padding-right: 12px;
    line-height: 100%;
    /* 录制态提示文案较长：保证单行显示，超宽截断而不折行溢出 */
    white-space: nowrap;
    overflow: hidden;
    font-family: var(--font-family);
    font-size: var(--font-size);
    text-align: left;
    background-color: var(--control-background);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
}

.shortcut-recorder:hover {
    background-color: var(--control-background-hover);
}

/* 录制态复用输入框的聚焦观感（WKWebView 中点击 button 不获焦，:focus 覆盖不到录制态） */
.shortcut-recorder:focus,
.shortcut-recorder.recording {
    outline: none;
    background-color: var(--input-text-background-focus);
    border-bottom-width: var(--input-text-border-bottom-width-focus);
    border-bottom-color: var(--accent);
}

.shortcut-recorder.recording {
    color: var(--accent-text);
}
</style>
