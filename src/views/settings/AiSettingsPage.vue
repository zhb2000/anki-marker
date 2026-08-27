<script setup lang="ts">
import { ElSwitch } from 'element-plus';

import { FluentInput, FluentPasswordInput, FluentSettingCard } from '../../fluent-controls';
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
        <h2 class="group-title">AI 优选释义</h2>
        <div class="card-list">
            <FluentSettingCard header="启用 AI 优选释义"
                description="按句子语境从多本词典中优选释义，需自行配置 LLM API" setting-id="llmEnabled">
                <template #header-extra>
                    <ResetButton @click="store.reset('llmEnabled')" />
                </template>
                <ElSwitch v-model="state.llmEnabled" />
            </FluentSettingCard>
            <template v-if="state.llmEnabled">
                <FluentSettingCard header="API 地址" setting-id="llmBaseUrl">
                    <template #header-extra>
                        <ResetButton @click="store.reset('llmBaseUrl')" />
                    </template>
                    <FluentInput class="card-input" placeholder="如 https://api.deepseek.com"
                        v-model="state.llmBaseUrl" />
                </FluentSettingCard>
                <FluentSettingCard header="API Key" setting-id="llmApiKey">
                    <template #header-extra>
                        <ResetButton @click="store.reset('llmApiKey')" />
                    </template>
                    <FluentPasswordInput class="card-input" placeholder="请输入 API Key（仅保存在本地配置）"
                        v-model="state.llmApiKey" />
                </FluentSettingCard>
                <FluentSettingCard header="模型" setting-id="llmModel">
                    <template #header-extra>
                        <ResetButton @click="store.reset('llmModel')" />
                    </template>
                    <FluentInput class="card-input" placeholder="如 deepseek-v4-flash" v-model="state.llmModel" />
                </FluentSettingCard>
                <FluentSettingCard header="思考强度" setting-id="llmReasoningEffort">
                    <template #header-extra>
                        <ResetButton @click="store.reset('llmReasoningEffort')" />
                    </template>
                    <FluentInput class="card-input" placeholder="留空则不传参；常见取值 low / medium / high"
                        v-model="state.llmReasoningEffort" />
                </FluentSettingCard>
            </template>
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
