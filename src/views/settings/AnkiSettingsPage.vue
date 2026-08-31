<script setup lang="ts">
import { FluentInput, FluentSettingCard, FluentToggleSwitch } from '../../fluent-controls';
import { ResetButton } from '../../components';
import { TEXT_SETTING_FALLBACKS } from '../../logics/config';
import { useSettingsStore } from '../../logics/settings-store';
import { createSettingInputBinder } from '../../logics/setting-input';
import { useHighlight } from './useHighlight';

const store = useSettingsStore();
const state = store.state;
const { bind } = createSettingInputBinder(store);

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();
</script>

<template>
    <div class="settings-page">
        <h2 class="group-title">Anki</h2>
        <div class="card-list">
            <FluentSettingCard header="AnkiConnect 服务" setting-id="ankiConnectURL"
                :description="`AnkiConnect 插件的服务地址，留空使用默认 ${TEXT_SETTING_FALLBACKS.ankiConnectURL}`">
                <template #header-extra>
                    <ResetButton setting-key="ankiConnectURL" />
                </template>
                <FluentInput class="card-input" :placeholder="`默认：${TEXT_SETTING_FALLBACKS.ankiConnectURL}`"
                    v-bind="bind('ankiConnectURL')" />
            </FluentSettingCard>
            <FluentSettingCard header="将划词结果添加到哪个牌组" setting-id="deckName"
                :description="`留空使用默认“${TEXT_SETTING_FALLBACKS.deckName}”`">
                <template #header-extra>
                    <ResetButton setting-key="deckName" />
                </template>
                <FluentInput class="card-input" :placeholder="`默认：${TEXT_SETTING_FALLBACKS.deckName}`"
                    v-bind="bind('deckName')" />
            </FluentSettingCard>
            <FluentSettingCard header="使用的笔记模板名称" setting-id="modelName"
                :description="`留空使用默认“${TEXT_SETTING_FALLBACKS.modelName}”`">
                <template #header-extra>
                    <ResetButton setting-key="modelName" />
                </template>
                <FluentInput class="card-input" :placeholder="`默认：${TEXT_SETTING_FALLBACKS.modelName}`"
                    v-bind="bind('modelName')" />
            </FluentSettingCard>
        </div>

        <h2 class="group-title">高级</h2>
        <div class="card-list">
            <FluentSettingCard header="自动启动 Anki"
                description="添加笔记时若 Anki 未运行，将自动启动 Anki 并等待其就绪" setting-id="autoLaunchAnki">
                <template #header-extra>
                    <ResetButton setting-key="autoLaunchAnki" />
                </template>
                <FluentToggleSwitch v-model="state.autoLaunchAnki" />
            </FluentSettingCard>
            <FluentSettingCard header="应用启动时启动 Anki" description="应用启动时自动启动 Anki，无需等到添加笔记"
                setting-id="launchAnkiOnAppStart">
                <template #header-extra>
                    <ResetButton setting-key="launchAnkiOnAppStart" />
                </template>
                <FluentToggleSwitch v-model="state.launchAnkiOnAppStart" />
            </FluentSettingCard>
            <FluentSettingCard header="Anki 可执行文件路径" setting-id="ankiExecutablePath"
                description="留空时自动检测 Anki 的安装位置">
                <template #header-extra>
                    <ResetButton setting-key="ankiExecutablePath" />
                </template>
                <FluentInput class="card-input" placeholder="留空自动检测"
                    v-bind="bind('ankiExecutablePath')" />
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
    width: min(400px, 100%);
}
</style>
