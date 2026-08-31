<script setup lang="ts">
import { computed, ref } from 'vue';
import { FluentButton, FluentInput, FluentPasswordInput, FluentSettingCard, FluentToggleSwitch } from '../../fluent-controls';
import { ModelListDialog, ResetButton } from '../../components';
import {
    LLM_DEFAULT_MAX_TOKENS,
    LlmError,
    fetchAvailableModels,
    testLlmConnection,
    type RemoteModelInfo,
} from '../../logics/llm';
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

// #region 模型列表拉取（“获取模型列表”按钮 + ModelListDialog）
const modelDialogOpen = ref(false);
const modelListLoading = ref(false);
const modelListError = ref<string | null>(null);
const modelList = ref<RemoteModelInfo[]>([]);

/**
 * 会话内缓存：key = `API 地址|API Key`（trim 后），地址或 Key 变更后自动失效重拉。
 * 仅存内存不进 config.toml（列表易过期且可能很大），应用重启后自然清空。
 */
const modelListCache = new Map<string, RemoteModelInfo[]>();
let modelListAbort: AbortController | null = null;

const fetchModelsDisabled = computed(() =>
    llmConfigDisabled.value
    || state.llmBaseUrl.trim().length === 0
    || state.llmApiKey.trim().length === 0
);

/** 按钮不可用时的悬浮说明（必填项已由各卡片内的“必填”提示指明来源） */
const fetchModelsTitle = computed(() =>
    fetchModelsDisabled.value && !llmConfigDisabled.value ? '请先填写 API 地址与 API Key' : undefined
);

function modelListCacheKey(): string {
    return `${state.llmBaseUrl.trim()}|${state.llmApiKey.trim()}`;
}

function openModelDialog(): void {
    modelDialogOpen.value = true;
    const cached = modelListCache.get(modelListCacheKey());
    if (cached != null) {
        // 命中缓存直接展示（弹窗内可“重新获取”强制刷新）
        modelList.value = cached;
        modelListError.value = null;
        modelListLoading.value = false;
    } else {
        void refreshModelList();
    }
}

async function refreshModelList(): Promise<void> {
    modelListAbort?.abort();
    const controller = new AbortController();
    modelListAbort = controller;
    modelListLoading.value = true;
    modelListError.value = null;
    try {
        const models = await fetchAvailableModels(
            { baseUrl: state.llmBaseUrl, apiKey: state.llmApiKey },
            controller.signal
        );
        modelListCache.set(modelListCacheKey(), models);
        modelList.value = models;
        modelListLoading.value = false;
    } catch (error) {
        if (controller.signal.aborted) {
            return; // 被新一次拉取或关闭动作取代，静默丢弃本次结果
        }
        modelListLoading.value = false;
        modelListError.value = error instanceof LlmError ? describeFetchModelsError(error) : String(error);
    } finally {
        if (modelListAbort === controller) {
            modelListAbort = null;
        }
    }
}

/** 将 LlmError 映射为面向用户的错误文案；HTTP 类保留上游返回原文便于定位 */
function describeFetchModelsError(error: LlmError): string {
    if (error.kind === 'http') {
        if (error.status === 401 || error.status === 403) {
            return `鉴权失败（HTTP ${error.status}），请检查 API Key 是否正确`;
        }
        if (error.status === 404 || error.status === 405) {
            return `该服务未提供模型列表接口（HTTP ${error.status}），可直接手动输入模型名`;
        }
        return error.message;
    }
    if (error.kind === 'timeout') {
        return '请求超时，请检查 API 地址是否可达';
    }
    if (error.kind === 'network') {
        return `网络请求失败：${error.message}`;
    }
    return error.message;
}

/** 弹窗内点选模型：输入框此时不处于编辑态（点按钮时已 blur 提交），直接写 store 走既有提交链路 */
function applyModel(id: string): void {
    state.llmModel = id;
    modelDialogOpen.value = false;
}
// #endregion

// #region 测试连接（常驻卡片，发送一条极小的真实补全验证可用性）
interface TestConnectionState {
    phase: 'idle' | 'testing' | 'success' | 'fail';
    /** 成功时的请求耗时（毫秒） */
    latencyMs?: number;
    /** 失败时的错误文案（含上游原文） */
    message?: string;
}

const testState = ref<TestConnectionState>({ phase: 'idle' });
let testAbort: AbortController | null = null;

const testConnectionDescription = computed(() => {
    switch (testState.value.phase) {
        case 'testing':
            return '正在测试连接…';
        case 'success':
            return `连接成功，耗时 ${testState.value.latencyMs} ms`;
        case 'fail':
            return testState.value.message ?? '测试失败';
        default:
            return '向上游发送一条极小的测试消息，验证 API 地址、Key 与模型真实可用';
    }
});

const testConnectionLabel = computed(() =>
    testState.value.phase === 'testing' ? '测试中…' : '测试连接'
);

const testConnectionDisabled = computed(() =>
    llmConfigDisabled.value
    || testState.value.phase === 'testing'
    || state.llmBaseUrl.trim().length === 0
    || state.llmApiKey.trim().length === 0
    || state.llmModel.trim().length === 0
);

const testConnectionTitle = computed(() => {
    if (llmConfigDisabled.value || testState.value.phase === 'testing') {
        return undefined;
    }
    return testConnectionDisabled.value ? '请先填写 API 地址、API Key 与模型' : undefined;
});

async function runConnectionTest(): Promise<void> {
    if (testState.value.phase === 'testing') {
        return;
    }
    testAbort?.abort();
    const controller = new AbortController();
    testAbort = controller;
    testState.value = { phase: 'testing' };
    try {
        const latencyMs = await testLlmConnection(
            { baseUrl: state.llmBaseUrl, apiKey: state.llmApiKey, model: state.llmModel },
            controller.signal
        );
        if (controller.signal.aborted) {
            return;
        }
        testState.value = { phase: 'success', latencyMs };
    } catch (error) {
        if (controller.signal.aborted) {
            return;
        }
        const message = error instanceof LlmError ? describeTestError(error) : String(error);
        testState.value = { phase: 'fail', message };
    } finally {
        if (testAbort === controller) {
            testAbort = null;
        }
    }
}

function describeTestError(error: LlmError): string {
    if (error.kind === 'timeout') {
        return '请求超时（15 秒），请检查 API 地址与模型是否正确';
    }
    if (error.kind === 'network') {
        return `网络请求失败：${error.message}`;
    }
    // http 与 bad_response：message 已含上游错误原文（如 key 无效/额度不足/模型不存在）
    return error.message;
}
// #endregion
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
                <div class="model-actions">
                    <FluentInput class="card-input" placeholder="如 deepseek-v4-flash"
                        v-bind="bind('llmModel')" :disabled="llmConfigDisabled" />
                    <FluentButton class="action-button" :disabled="fetchModelsDisabled" :title="fetchModelsTitle"
                        @click="openModelDialog">获取模型列表</FluentButton>
                </div>
                <div v-if="isRequiredMissing('llmModel')" class="required-hint">必填</div>
            </FluentSettingCard>
            <FluentSettingCard header="测试连接" setting-id="llmTestConnection"
                :disabled="llmConfigDisabled" :description="llmConfigDisabledReason ?? testConnectionDescription">
                <FluentButton class="action-button" :disabled="testConnectionDisabled" :title="testConnectionTitle"
                    @click="runConnectionTest">{{ testConnectionLabel }}</FluentButton>
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
        <ModelListDialog :open="modelDialogOpen" :loading="modelListLoading" :error="modelListError"
            :models="modelList" :current-model="state.llmModel.trim()"
            @close="modelDialogOpen = false" @refresh="refreshModelList" @select="applyModel" />
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

/* 模型卡片操作区：输入框 + “获取模型列表”按钮同行（空间不足时随卡片整体换行） */
.model-actions {
    display: flex;
    align-items: center;
    gap: 8px;
}

/* 卡片内的动作按钮：高度与 card-input 对齐 */
.action-button {
    height: 32px;
    padding-left: 12px;
    padding-right: 12px;
    flex-shrink: 0;
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
