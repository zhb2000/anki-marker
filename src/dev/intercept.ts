/**
 * 开发夹具的安装入口：把 HTTP 与命令拦截器挂到 window 上，供业务侧的薄包装调用。
 *
 * 为什么不在 Tauri IPC 层打桩：原生 dev 窗口里 __TAURI_INTERNALS__.invoke 不可写，
 * 无法像浏览器模式那样包装。把拦截点放在 src/logics/net.ts 与 utils.invoke 之后，
 * 同一套 AI / 配置夹具就能同时服务 tauri dev、dev:ai 与 dev:browser。
 */
import { invoke as coreInvoke } from '@tauri-apps/api/core';
import { watch } from 'vue';

import { clearAiPickCache } from '../logics/aiPick';
import type { DevFetchInterceptor } from '../logics/net';
import { clearLlmCompatCache } from '../logics/llm';
import type { DevInvokeInterceptor, DevInvokeResult } from '../logics/utils';
import { beginMockActivity } from './activity';
import { effectiveLlmScenario } from './effective';
import { currentDevEnvironment, DEV_ENVIRONMENT_LABELS } from './environment';
import { recordMockHttp } from './inspect';
import { handleLlmMock, isLlmMockUrl, MOCK_LLM_API_KEY, MOCK_LLM_BASE_URL, MOCK_LLM_MODEL } from './mock/llm';
import { devSettings } from './state';

/** 模拟 AI 连接时一并覆盖的配置键；写入时也把这些键剔除，避免假配置落进真实文件 */
const LLM_CONFIG_KEYS = [
    'llmEnabled', 'llmBaseUrl', 'llmApiKey', 'llmModel',
    'llmMaxTokens', 'llmReasoningEffort', 'llmTemperature',
] as const;

/** 当前是否应该把真实配置替换成模拟 AI 连接（真实请求场景下不注入假配置） */
function mockLlmConfigActive(): boolean {
    return devSettings.mockLlmConfig
        && effectiveLlmScenario(devSettings, currentDevEnvironment()) !== 'real';
}

function parseJsonBody(body: BodyInit | null | undefined): unknown {
    if (typeof body !== 'string') {
        return undefined;
    }
    try {
        return JSON.parse(body) as unknown;
    } catch {
        return undefined;
    }
}

function requestUrl(input: string | URL | Request): URL {
    if (input instanceof Request) {
        return new URL(input.url);
    }
    return input instanceof URL ? input : new URL(input, window.location.href);
}

const devFetch: DevFetchInterceptor = (input, init) => {
    const url = requestUrl(input);
    const method = (init?.method ?? (input instanceof Request ? input.method : 'GET')).toUpperCase();
    if (!isLlmMockUrl(url, method)) {
        return undefined;
    }
    const body = parseJsonBody(init?.body);
    const signal = init?.signal ?? (input instanceof Request ? input.signal : undefined);
    const scenario = effectiveLlmScenario(devSettings, currentDevEnvironment());
    const done = beginMockActivity();
    const startedAt = performance.now();
    try {
        const response = handleLlmMock({ url, method, body, signal, onDone: done });
        recordMockHttp(method, url, response.status, performance.now() - startedAt, scenario);
        return response;
    } catch (error) {
        done();
        recordMockHttp(method, url, 0, performance.now() - startedAt, scenario, error);
        throw error;
    }
};

const devInvoke: DevInvokeInterceptor = async (cmd, args): Promise<DevInvokeResult> => {
    if (cmd === 'read_config' && mockLlmConfigActive()) {
        const real = await coreInvoke<Record<string, unknown>>('read_config');
        return {
            handled: true,
            value: {
                ...real,
                llmEnabled: true,
                llmBaseUrl: MOCK_LLM_BASE_URL,
                llmApiKey: MOCK_LLM_API_KEY,
                llmModel: MOCK_LLM_MODEL,
            },
        };
    }
    if (cmd === 'commit_config' && mockLlmConfigActive()) {
        const modified = { ...((args?.modified as Record<string, unknown> | undefined) ?? {}) };
        for (const key of LLM_CONFIG_KEYS) {
            delete modified[key];
        }
        if (Object.keys(modified).length === 0) {
            return { handled: true, value: null };
        }
        const value = await coreInvoke('commit_config', { ...(args ?? {}), modified });
        return { handled: true, value };
    }
    return { handled: false };
};

/** 修改模拟配置后，让已加载的 Config 立即重新读取（read_config 会应用/移除模拟覆盖） */
async function reloadConfigForDev(): Promise<void> {
    try {
        const { getConfig } = await import('../logics/globals');
        const config = await getConfig();
        await config.reload();
    } catch (error) {
        console.warn('[dev] 重新读取配置失败', error);
    }
}

/** 清掉 AI 结果缓存与参数兼容缓存，让下一次请求按当前场景重新走 */
export function resetMockState(): void {
    clearAiPickCache();
    clearLlmCompatCache();
    console.info('[dev] 已重置 AI 模拟状态（结果缓存与参数兼容缓存）');
}

/** 安装 AI 场景拦截器；只在 dev 功能初始化时调用一次 */
export function installDevInterceptors(): void {
    const target = window as unknown as {
        __devFetchInterceptor?: DevFetchInterceptor;
        __devInvokeInterceptor?: DevInvokeInterceptor;
    };
    target.__devFetchInterceptor = devFetch;
    target.__devInvokeInterceptor = devInvoke;
    if (window.__dev != null) {
        window.__dev.resetMock = resetMockState;
    }

    const environment = currentDevEnvironment();
    const scenario = effectiveLlmScenario(devSettings, environment);
    if (scenario === 'real') {
        console.info(`[dev] LLM 场景为真实请求（${DEV_ENVIRONMENT_LABELS[environment]}）：会使用配置中的真实 API Key 联网`);
    } else {
        console.info(`[dev] LLM 请求由夹具模拟（${DEV_ENVIRONMENT_LABELS[environment]} / ${scenario}）；要联网请把“LLM / AI 优选”切到“真实请求”`);
    }

    watch(
        () => [devSettings.llmScenario, devSettings.mockLlmConfig] as const,
        () => {
            clearAiPickCache();
            clearLlmCompatCache();
            // 可能是打开也可能关闭模拟配置，两个方向都要让已加载的 Config 重新读取
            void reloadConfigForDev();
            const environment = currentDevEnvironment();
            const scenario = effectiveLlmScenario(devSettings, environment);
            console.info(`[dev] AI 场景已切换：${DEV_ENVIRONMENT_LABELS[environment]} / ${scenario}（模拟配置：${devSettings.mockLlmConfig ? '开' : '关'}）`);
        },
    );
}
