<script setup lang="ts">
import { computed, watch } from 'vue';
import { ElSwitch } from 'element-plus';

import * as api from '../../tauri-api';
import { FluentSelect, FluentSettingCard, type FluentSelectOption } from '../../fluent-controls';
import { ResetButton } from '../../components';
import { useSettingsStore } from '../../logics/settings-store';
import { setThemeMode } from '../../logics/theme';
import { useHighlight } from './useHighlight';

const store = useSettingsStore();
const state = store.state;

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();

/** 主题下拉的选项：跟随系统 / 浅色 / 深色 */
const themeOptions: FluentSelectOption[] = [
    { value: 'system', label: '跟随系统' },
    { value: 'light', label: '浅色' },
    { value: 'dark', label: '深色' },
];

/** 后台运行图标下拉的选项：Dock 栏图标 / 菜单栏图标 / 都不显示 */
const backgroundIconOptions: FluentSelectOption[] = [
    { value: 'dock', label: 'Dock 栏图标' },
    { value: 'menu-bar', label: '菜单栏图标' },
    { value: 'none', label: '都不显示' },
];

// 主题切换即时生效：全窗口共享同一 DOM，setThemeMode 直接切换 dark class，无需等待保存
watch(() => state.theme, theme => setThemeMode(theme));

/** 是否为 macOS（窗口行为组仅 macOS 展示，与现设置页一致） */
const isMacOS = computed(() => api.os.type() === 'macos');
</script>

<template>
    <div class="settings-page">
        <h2 class="group-title">外观</h2>
        <div class="card-list">
            <FluentSettingCard header="主题" setting-id="theme">
                <template #header-extra>
                    <ResetButton @click="store.reset('theme')" />
                </template>
                <FluentSelect class="card-input" :options="themeOptions" v-model="state.theme" />
            </FluentSettingCard>
        </div>

        <template v-if="isMacOS">
            <h2 class="group-title">窗口</h2>
            <div class="card-list">
                <FluentSettingCard header="关闭窗口后保持后台运行"
                    description="关闭窗口后应用将在后台继续运行，可通过 Dock 图标、菜单栏图标或全局快捷键再次打开"
                    setting-id="keepRunningOnClose">
                    <template #header-extra>
                        <ResetButton @click="store.reset('keepRunningOnClose')" />
                    </template>
                    <ElSwitch v-model="state.keepRunningOnClose" />
                </FluentSettingCard>
                <FluentSettingCard v-if="state.keepRunningOnClose" header="后台运行时显示图标"
                    description="选择窗口关闭后（后台运行期间）应用图标的显示位置；窗口打开时图标始终显示在 Dock 栏"
                    setting-id="backgroundIcon">
                    <template #header-extra>
                        <ResetButton @click="store.reset('backgroundIcon')" />
                    </template>
                    <FluentSelect class="card-input" :options="backgroundIconOptions"
                        v-model="state.backgroundIcon" />
                </FluentSettingCard>
            </div>
        </template>
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
</style>
