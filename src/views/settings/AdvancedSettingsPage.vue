<script setup lang="ts">
import { onBeforeMount, computed, ref } from 'vue';

import * as api from '../../tauri-api';
import * as cfg from '../../logics/config';
import * as globals from '../../logics/globals';
import { FluentButton, FluentSettingCard } from '../../fluent-controls';
import { setThemeMode } from '../../logics/theme';
import { useSettingsStore } from '../../logics/settings-store';
import { useHighlight } from './useHighlight';

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();

const store = useSettingsStore();

/** 已修改（偏离默认值）的设置项数量；为 0 时恢复默认按钮无意义，应置灰 */
const modifiedCount = computed(() => cfg.CONFIG_KEYS.filter(key => store.isModified(key)).length);

/** 配置文件对象（只读使用：path / portable）；shell 已完成 store.init，此处 getConfig 直接命中缓存 */
const config = ref<cfg.Config | null>(null);

onBeforeMount(async () => {
    config.value = await globals.getConfig();
});

/** 点击打开配置文件按钮 */
async function handleOpenFileClick() {
    if (config.value == null) {
        return;
    }
    try {
        await cfg.openFile(config.value.path);
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '打开文件失败', kind: 'error' });
    }
}

/** 点击恢复默认设置按钮：确认后把全部设置写回默认值（写 state 即触发自动保存） */
async function handleResetAllClick() {
    const confirmed = await api.dialog.confirm(
        `将把 ${modifiedCount.value} 项已修改的设置恢复为默认值，此操作不可撤销。`,
        {
            title: '恢复默认设置',
            kind: 'warning',
            okLabel: '恢复默认设置',
            cancelLabel: '取消',
        },
    );
    if (!confirmed) {
        return;
    }
    store.resetAll();
    // 主题不在本页展示，通用页未挂载时其 watch 不会生效，这里显式同步一次
    setThemeMode(store.state.theme);
}

/** 点击打开配置文件所在目录按钮 */
async function handleShowInExplorerClick() {
    if (config.value == null) {
        return;
    }
    try {
        await cfg.showInExplorer(config.value.path);
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '打开目录失败', kind: 'error' });
    }
}
</script>

<template>
    <div class="settings-page" v-if="config != null">
        <h2 class="group-title">配置文件</h2>
        <div class="card-list">
            <FluentSettingCard header="安装/便携模式" setting-id="portable-mode">
                <span class="value-text">{{ config.portable ? '便携模式' : '安装模式' }}</span>
            </FluentSettingCard>
            <FluentSettingCard header="配置文件路径" setting-id="config-path">
                <span class="value-text file-path">{{ config.path }}</span>
            </FluentSettingCard>
            <FluentSettingCard header="打开配置文件" setting-id="open-config-file">
                <FluentButton @click="handleOpenFileClick">打开文件</FluentButton>
                <FluentButton v-if="(['windows', 'macos', 'linux'] as api.os.OsType[]).includes(api.os.type())"
                    @click="handleShowInExplorerClick">
                    打开目录
                </FluentButton>
            </FluentSettingCard>
        </div>

        <h2 class="group-title">重置</h2>
        <div class="card-list">
            <FluentSettingCard header="恢复全部默认设置"
                :description="modifiedCount > 0
                    ? `当前有 ${modifiedCount} 项设置与默认值不同；恢复立即生效且不可撤销`
                    : '所有设置均为默认值'"
                setting-id="reset-all">
                <FluentButton @click="handleResetAllClick" :disabled="modifiedCount === 0">恢复默认设置
                </FluentButton>
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

.value-text {
    user-select: none;
    cursor: default;
}

/* 配置文件路径可选中复制（沿用旧 .file-path 的样式思路） */
.file-path {
    user-select: text;
    cursor: text;
    overflow-wrap: break-word;
    word-break: break-all;
    text-align: right;
}
</style>
