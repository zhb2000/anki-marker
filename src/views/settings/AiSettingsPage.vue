<script setup lang="ts">
import { computed } from 'vue';
import { FluentInput, FluentPasswordInput, FluentSettingCard, FluentToggleSwitch } from '../../fluent-controls';
import { ResetButton } from '../../components';
import { LLM_DEFAULT_MAX_TOKENS } from '../../logics/llm';
import { useSettingsStore } from '../../logics/settings-store';
import { createSettingInputBinder } from '../../logics/setting-input';
import { useHighlight } from './useHighlight';

const store = useSettingsStore();
const state = store.state;
const { bind } = createSettingInputBinder(store);

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();

/** LLM 子配置项未启用时禁用：卡片常显（搜索跳转锚点始终有效、布局稳定），仅禁用交互 */
const llmConfigDisabled = computed(() => !state.llmEnabled);

/**
 * 启用 AI 优选后，三项必填连接配置（API 地址 / API Key / 模型）是否缺失。
 * 任一缺失时功能不会就绪（见 llm.ts 的 isLlmReady），对应卡片内显示必填提示，
 * 避免用户把必填项当作可选配置。
 */
function isRequiredMissing(key: 'llmBaseUrl' | 'llmApiKey' | 'llmModel'): boolean {
    return state.llmEnabled && state[key].trim().length === 0;
}

/** 子配置卡禁用时的原因说明（启用后各卡无 description，恢复为 undefined） */
const llmConfigDisabledReason = computed(() =>
    llmConfigDisabled.value ? '需先开启“启用 AI 优选释义”' : undefined
);

/** 最大生成 Token 数卡片的常驻说明（禁用时让位于禁用原因） */
const maxTokensDescription = `单次请求的生成上限；思考模型的思维链计入此配额，释义被截断或为空时可调大；留空使用默认 ${LLM_DEFAULT_MAX_TOKENS}`;

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
                    <ResetButton setting-key="llmEnabled" />
                </template>
                <FluentToggleSwitch v-model="state.llmEnabled" />
            </FluentSettingCard>
            <FluentSettingCard header="API 地址" setting-id="llmBaseUrl"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason">
                <template #header-extra>
                    <ResetButton setting-key="llmBaseUrl" :disabled="llmConfigDisabled" />
                </template>
                <FluentInput class="card-input" placeholder="如 https://api.deepseek.com"
                    v-bind="bind('llmBaseUrl')" :disabled="llmConfigDisabled" />
                <div v-if="isRequiredMissing('llmBaseUrl')" class="required-hint">必填</div>
            </FluentSettingCard>
            <FluentSettingCard header="API Key" setting-id="llmApiKey"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason ?? '仅保存在本地配置文件'">
                <template #header-extra>
                    <ResetButton setting-key="llmApiKey" :disabled="llmConfigDisabled" />
                </template>
                <FluentPasswordInput class="card-input" placeholder="请输入 API Key"
                    v-bind="bind('llmApiKey')" :disabled="llmConfigDisabled" />
                <div v-if="isRequiredMissing('llmApiKey')" class="required-hint">必填</div>
            </FluentSettingCard>
            <FluentSettingCard header="模型" setting-id="llmModel"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason">
                <template #header-extra>
                    <ResetButton setting-key="llmModel" :disabled="llmConfigDisabled" />
                </template>
                <FluentInput class="card-input" placeholder="如 deepseek-v4-flash"
                    v-bind="bind('llmModel')" :disabled="llmConfigDisabled" />
                <div v-if="isRequiredMissing('llmModel')" class="required-hint">必填</div>
            </FluentSettingCard>
            <FluentSettingCard header="最大生成 Token 数" setting-id="llmMaxTokens"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason ?? maxTokensDescription">
                <template #header-extra>
                    <ResetButton setting-key="llmMaxTokens" :disabled="llmConfigDisabled" />
                </template>
                <FluentInput class="card-input" :placeholder="`默认：${LLM_DEFAULT_MAX_TOKENS}`"
                    v-bind="bind('llmMaxTokens')" :disabled="llmConfigDisabled" />
            </FluentSettingCard>
            <FluentSettingCard header="思考强度" setting-id="llmReasoningEffort"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason ?? reasoningEffortDescription">
                <template #header-extra>
                    <ResetButton setting-key="llmReasoningEffort" :disabled="llmConfigDisabled" />
                </template>
                <FluentInput class="card-input" placeholder="留空则不传参"
                    v-bind="bind('llmReasoningEffort')" :disabled="llmConfigDisabled" />
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

/* 必填提示：与输入框同行显示（空间不足时随操作区换行），语义色与状态文本一致 */
.required-hint {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--warning-text-color);
    white-space: nowrap;
    user-select: none;
    cursor: default;
}
</style>
