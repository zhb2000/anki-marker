<script setup lang="ts">
import { computed } from 'vue';
import { ElSwitch } from 'element-plus';

import { FluentInput, FluentPasswordInput, FluentSettingCard } from '../../fluent-controls';
import { ResetButton } from '../../components';
import { useSettingsStore } from '../../logics/settings-store';
import { useHighlight } from './useHighlight';

const store = useSettingsStore();
const state = store.state;

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();

/** LLM 子配置项未启用时禁用：卡片常显（搜索跳转锚点始终有效、布局稳定），仅禁用交互 */
const llmConfigDisabled = computed(() => !state.llmEnabled);

/** 子配置卡禁用时的原因说明（启用后各卡无 description，恢复为 undefined） */
const llmConfigDisabledReason = computed(() =>
    llmConfigDisabled.value ? '需先开启“启用 AI 优选释义”' : undefined
);

/** 最大生成 Token 数卡片的常驻说明（禁用时让位于禁用原因） */
const maxTokensDescription = '单次请求的生成上限；思考模型的思维链计入此配额，释义被截断或为空时可调大';

/** 思考强度卡片的常驻说明（禁用时让位于禁用原因） */
const reasoningEffortDescription = '常见取值 low / medium / high，实际支持因所配服务而异；留空则不传参';
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
            <FluentSettingCard header="API 地址" setting-id="llmBaseUrl"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason">
                <template #header-extra>
                    <ResetButton :disabled="llmConfigDisabled" @click="store.reset('llmBaseUrl')" />
                </template>
                <FluentInput class="card-input" placeholder="如 https://api.deepseek.com"
                    v-model="state.llmBaseUrl" :disabled="llmConfigDisabled" />
            </FluentSettingCard>
            <FluentSettingCard header="API Key" setting-id="llmApiKey"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason">
                <template #header-extra>
                    <ResetButton :disabled="llmConfigDisabled" @click="store.reset('llmApiKey')" />
                </template>
                <FluentPasswordInput class="card-input" placeholder="请输入 API Key（仅保存在本地配置）"
                    v-model="state.llmApiKey" :disabled="llmConfigDisabled" />
            </FluentSettingCard>
            <FluentSettingCard header="模型" setting-id="llmModel"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason">
                <template #header-extra>
                    <ResetButton :disabled="llmConfigDisabled" @click="store.reset('llmModel')" />
                </template>
                <FluentInput class="card-input" placeholder="如 deepseek-v4-flash" v-model="state.llmModel"
                    :disabled="llmConfigDisabled" />
            </FluentSettingCard>
            <FluentSettingCard header="最大生成 Token 数" setting-id="llmMaxTokens"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason ?? maxTokensDescription">
                <template #header-extra>
                    <ResetButton :disabled="llmConfigDisabled" @click="store.reset('llmMaxTokens')" />
                </template>
                <FluentInput class="card-input" placeholder="留空使用默认 8192"
                    v-model="state.llmMaxTokens" :disabled="llmConfigDisabled" />
            </FluentSettingCard>
            <FluentSettingCard header="思考强度" setting-id="llmReasoningEffort"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason ?? reasoningEffortDescription">
                <template #header-extra>
                    <ResetButton :disabled="llmConfigDisabled" @click="store.reset('llmReasoningEffort')" />
                </template>
                <FluentInput class="card-input" placeholder="留空则不传参；常见取值 low / medium / high"
                    v-model="state.llmReasoningEffort" :disabled="llmConfigDisabled" />
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
