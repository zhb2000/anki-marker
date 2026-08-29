<script setup lang="ts">
import { computed, onActivated } from 'vue';

import * as api from '../../tauri-api';
import { FluentButton, FluentSettingCard, FluentToggleSwitch } from '../../fluent-controls';
import { ResetButton, ShortcutRecorder } from '../../components';
import { useSettingsStore } from '../../logics/settings-store';
import {
    shortcutError, accessibilityTrusted,
    checkAccessibilityTrust, requestAccessibilityTrust,
} from '../../logics/shortcut-status';
import { useHighlight } from './useHighlight';

const store = useSettingsStore();
const state = store.state;

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();

/** 是否为 macOS（全局快捷键目前仅支持 macOS） */
const isMacOS = computed(() => api.os.type() === 'macos');

/** 辅助功能权限状态显示文本 */
const accessibilityStatusText = computed(() => {
    if (accessibilityTrusted.value === true) {
        return '✓ 已授权';
    } else if (accessibilityTrusted.value === false) {
        return '⚠️ 未授权';
    }
    return '检查中…';
});

/** 辅助功能权限状态文本的颜色（跟随浅色/深色主题的语义色变量） */
const accessibilityStatusClass = computed(() => {
    if (accessibilityTrusted.value === true) {
        return 'status-success';
    } else if (accessibilityTrusted.value === false) {
        return 'status-warning';
    }
    return '';
});

/** 是否显示快捷键冲突标识（仅在已设置快捷键且注册失败时显示） */
const showShortcutError = computed(() =>
    shortcutError.value != null && state.globalShortcut.length > 0
);

/** 快捷键变更（录制/清除）后由 store 自动保存配置；快捷键从无到有时权限状态行首次出现，顺带检查一次权限 */
function handleShortcutChange() {
    // v-model 的赋值先于本 handler 执行，此处 state.globalShortcut 已是最新值
    void checkAccessibilityTrust();
}

onActivated(() => {
    // KeepAlive 重新激活（返回设置页）时刷新一次权限状态
    void checkAccessibilityTrust();
});
</script>

<template>
    <div class="settings-page">
        <template v-if="isMacOS">
            <h2 class="group-title">划词</h2>
            <div class="card-list">
                <FluentSettingCard header="全局快捷键（录入句子）"
                    description="在任意应用中选中一段文字后按下此快捷键，所选文字将录入划词面板并自动分词。"
                    setting-id="globalShortcut">
                    <template #header-extra>
                        <ResetButton setting-key="globalShortcut" />
                    </template>
                    <ShortcutRecorder class="card-input shortcut-input" v-model="state.globalShortcut"
                        @update:model-value="handleShortcutChange" />
                    <div v-if="showShortcutError" class="status-text status-warning shortcut-error">
                        ⚠️ 快捷键注册失败（可能被其他应用占用）：{{ shortcutError }}
                    </div>
                </FluentSettingCard>
                <FluentSettingCard header="选词取句"
                    description="开启后，只需选中一个单词按下快捷键，即可自动录入该单词所在的整个句子并查询该单词；关闭则需选中完整句子录入。"
                    setting-id="wordToSentence">
                    <template #header-extra>
                        <ResetButton setting-key="wordToSentence" />
                    </template>
                    <FluentToggleSwitch v-model="state.wordToSentence" />
                </FluentSettingCard>
                <FluentSettingCard v-if="state.globalShortcut.length > 0" header="辅助功能权限"
                    :description="accessibilityTrusted === false ? '辅助功能未授权，划词功能可能无法使用。请点击上方“申请权限”按钮，并按系统提示授权本应用。' : undefined">
                    <span :class="accessibilityStatusClass" class="status-text">{{ accessibilityStatusText }}</span>
                    <FluentButton v-if="accessibilityTrusted === false" accent
                        @click="requestAccessibilityTrust">申请权限</FluentButton>
                    <FluentButton @click="checkAccessibilityTrust">检查</FluentButton>
                </FluentSettingCard>
            </div>
        </template>
        <div v-else class="platform-empty">该平台暂不支持全局快捷键</div>
    </div>
</template>

<style scoped>
.group-title {
    margin: 24px 0 6px 2px;
    padding: 0;
    font-size: 14px;
    font-weight: normal;
    opacity: 0.6;
    user-select: none;
    cursor: default;
}

.group-title:first-child {
    margin-top: 0;
}

.card-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
}

.card-input {
    width: min(320px, 100%);
}

/* ShortcutRecorder 自身不定高，由卡片操作区定高（沿用旧 .input-text 的 32px） */
.shortcut-input {
    height: 32px;
}

.status-text {
    user-select: none;
    cursor: default;
}

.status-success {
    color: var(--success-text-color);
}

.status-warning {
    color: var(--warning-text-color);
}

/* 快捷键冲突标识：与录入框拉开间距，独占一行 */
.shortcut-error {
    margin-top: 4px;
}

.platform-empty {
    padding: 24px 2px;
    font-size: 14px;
    opacity: 0.6;
    user-select: none;
    cursor: default;
}
</style>
