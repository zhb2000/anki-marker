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
import { isSettingAvailable } from '../../logics/settings-registry';
import { useHighlight } from './useHighlight';

const store = useSettingsStore();
const state = store.state;

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();

/** 当前平台：辅助功能权限卡片的可见性与搜索结果共用同一份判断（见 settings-registry 的 platforms） */
const platform = api.os.type();

/**
 * 是否已设置全局快捷键。选词取句与辅助功能权限都只服务于“按下快捷键取词”这条链路，
 * 未设置快捷键时它们不生效：卡片常显（搜索锚点始终有效、布局稳定）但整体禁用，
 * 与 AI 设置页里关闭“启用 AI 优选释义”后各子配置项的处置一致
 */
const shortcutSet = computed(() => state.globalShortcut.length > 0);

/** 依赖项禁用时的原因说明（占用 description，替代各自的常规说明） */
const dependencyHint = '需先设置全局快捷键';

/** 选词取句说明：禁用时让位于禁用原因 */
const wordToSentenceDescription = computed(() => shortcutSet.value
    ? '开启后，只需选中一个单词按下快捷键，即可自动录入该单词所在的整个句子并查询该单词；关闭则需选中完整句子录入。'
    : dependencyHint);

/** 辅助功能权限说明：禁用时让位于禁用原因，否则按授权状态给出下一步指引 */
const accessibilityDescription = computed(() => {
    if (!shortcutSet.value) {
        return dependencyHint;
    }
    return accessibilityTrusted.value === false
        ? '辅助功能未授权，划词功能可能无法使用。请点击“申请权限”按钮，在系统设置的辅助功能面板中授权本应用。'
        : '点击“打开系统设置”可随时前往辅助功能面板（移除本应用即可撤销授权），撤销后划词功能将不可用。';
});

/** 辅助功能权限状态显示文本：禁用态不加 ✓/⚠️ 前缀，避免未启用时仍像一条告警 */
const accessibilityStatusText = computed(() => {
    if (accessibilityTrusted.value === null) {
        return '检查中…';
    }
    const text = accessibilityTrusted.value ? '已授权' : '未授权';
    if (!shortcutSet.value) {
        return text;
    }
    return accessibilityTrusted.value ? `✓ ${text}` : `⚠️ ${text}`;
});

/** 辅助功能权限状态文本的颜色（跟随浅色/深色主题的语义色变量）；禁用态不染色 */
const accessibilityStatusClass = computed(() => {
    if (!shortcutSet.value) {
        return '';
    }
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

/** 快捷键变更（录制/清除）后由 store 自动保存配置；从无到有时依赖它的两张卡由禁用转为可用，顺带刷新一次权限状态 */
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
        <h2 class="group-title">划词</h2>
        <div class="card-list">
            <FluentSettingCard header="全局快捷键（录入句子）"
                description="在任意应用中选中一段文字后按下此快捷键，所选文字将录入划词面板并自动分词。"
                setting-id="globalShortcut">
                <template #header-extra>
                    <ResetButton setting-key="globalShortcut" />
                </template>
                <ShortcutRecorder class="shortcut-input" v-model="state.globalShortcut"
                    @update:model-value="handleShortcutChange" />
                <div v-if="showShortcutError" class="status-text status-warning shortcut-error">
                    ⚠️ 快捷键注册失败（可能被其他应用占用）：{{ shortcutError }}
                </div>
            </FluentSettingCard>
            <FluentSettingCard header="选词取句" :description="wordToSentenceDescription"
                :disabled="!shortcutSet" setting-id="wordToSentence">
                <template #header-extra>
                    <ResetButton setting-key="wordToSentence" :disabled="!shortcutSet" />
                </template>
                <FluentToggleSwitch v-model="state.wordToSentence" :disabled="!shortcutSet" />
            </FluentSettingCard>
            <!-- 辅助功能权限为 macOS 专属概念（Windows UIA / Linux AT-SPI 均无需授权）；
                 未设置快捷键时同样常显但禁用（与上面的选词取句一致） -->
            <FluentSettingCard v-if="isSettingAvailable('accessibility-permission', platform)"
                header="辅助功能权限" setting-id="accessibility-permission"
                :disabled="!shortcutSet" :description="accessibilityDescription">
                <span :class="accessibilityStatusClass" class="status-text">{{ accessibilityStatusText }}</span>
                <FluentButton v-if="accessibilityTrusted === false" accent :disabled="!shortcutSet"
                    @click="requestAccessibilityTrust">申请权限</FluentButton>
                <FluentButton v-else-if="accessibilityTrusted === true" :disabled="!shortcutSet"
                    @click="requestAccessibilityTrust">打开系统设置</FluentButton>
                <FluentButton :disabled="!shortcutSet" @click="checkAccessibilityTrust">检查</FluentButton>
            </FluentSettingCard>
        </div>
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
</style>
