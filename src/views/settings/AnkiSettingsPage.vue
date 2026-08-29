<script setup lang="ts">
import { FluentInput, FluentSettingCard, FluentToggleSwitch } from '../../fluent-controls';
import { ResetButton } from '../../components';
import { useSettingsStore } from '../../logics/settings-store';
import { useHighlight } from './useHighlight';

const store = useSettingsStore();
const state = store.state;

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();
</script>

<template>
    <div class="settings-page">
        <h2 class="group-title">Anki</h2>
        <div class="card-list">
            <FluentSettingCard header="AnkiConnect 服务" setting-id="ankiConnectURL">
                <template #header-extra>
                    <ResetButton @click="store.reset('ankiConnectURL')" />
                </template>
                <FluentInput class="card-input" placeholder="请输入 AnkiConnect 服务的 URL"
                    v-model="state.ankiConnectURL" />
            </FluentSettingCard>
            <FluentSettingCard header="将划词结果添加到哪个牌组" setting-id="deckName">
                <template #header-extra>
                    <ResetButton @click="store.reset('deckName')" />
                </template>
                <FluentInput class="card-input" placeholder="请输入牌组名称" v-model="state.deckName" />
            </FluentSettingCard>
            <FluentSettingCard header="使用的笔记模板名称" setting-id="modelName">
                <template #header-extra>
                    <ResetButton @click="store.reset('modelName')" />
                </template>
                <FluentInput class="card-input" placeholder="请输入笔记模板名称" v-model="state.modelName" />
            </FluentSettingCard>
        </div>

        <h2 class="group-title">高级</h2>
        <div class="card-list">
            <FluentSettingCard header="自动启动 Anki"
                description="添加笔记时若 Anki 未运行，将自动启动 Anki 并等待其就绪" setting-id="autoLaunchAnki">
                <template #header-extra>
                    <ResetButton @click="store.reset('autoLaunchAnki')" />
                </template>
                <FluentToggleSwitch v-model="state.autoLaunchAnki" />
            </FluentSettingCard>
            <FluentSettingCard header="应用启动时启动 Anki" description="应用启动时自动启动 Anki，无需等到添加笔记"
                setting-id="launchAnkiOnAppStart">
                <template #header-extra>
                    <ResetButton @click="store.reset('launchAnkiOnAppStart')" />
                </template>
                <FluentToggleSwitch v-model="state.launchAnkiOnAppStart" />
            </FluentSettingCard>
            <FluentSettingCard header="Anki 可执行文件路径" setting-id="ankiExecutablePath">
                <template #header-extra>
                    <ResetButton @click="store.reset('ankiExecutablePath')" />
                </template>
                <FluentInput class="card-input" placeholder="留空则自动检测 Anki 路径"
                    v-model="state.ankiExecutablePath" />
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

.card-input {
    height: 32px;
    width: min(320px, 100%);
}
</style>
