import { fetch } from '@tauri-apps/plugin-http';

/**
 * OpenAI 兼容的 chat completions 流式客户端。
 *
 * 传输层使用 `@tauri-apps/plugin-http` 的 `fetch`（走 Rust 侧网络栈，不受 WebView CORS
 * 限制；项目已开启 unsafe-headers，可设置 Authorization 头）。本模块不直接读取配置系统，
 * 所有连接参数均由调用方传入。
 */

/** LLM 请求所需的连接配置 */
export interface LlmRequestConfig {
    /**
     * OpenAI 兼容服务的 Base URL。
     * 允许带/不带末尾 '/'，允许已含 /v1 等路径；内部归一化后再拼接 /chat/completions。
     */
    baseUrl: string;
    /** API Key（BYOK，仅存本地） */
    apiKey: string;
    /** 模型名 */
    model: string;
    /**
     * 单次请求的最大生成 token 数（请求体中的 max_tokens）。
     * 注意思考模型（reasoning model）的思维链通常计入此配额，预算不足时正文会被截断甚至为空。
     */
    maxTokens: number;
    /** 推理强度；非空时以 OpenAI 风格 reasoning_effort 字段透传（统一转小写），空/undefined 不传 */
    reasoningEffort?: string;
}

/** 单条聊天消息 */
export type ChatMessage = { role: 'system' | 'user' | 'assistant'; content: string };

/** max_tokens 配置的内置默认值：足以容纳思考模型（如 DeepSeek V4 默认档位）的思维链加正文 */
export const LLM_DEFAULT_MAX_TOKENS = 8192;

/**
 * 解析用户配置的最大生成 token 数。
 * trim 后须为正整数（前导零容忍），空串/非数字/0/超安全整数范围均回退 LLM_DEFAULT_MAX_TOKENS。
 */
export function parseMaxTokens(raw: string): number {
    const trimmed = raw.trim();
    if (!/^\d+$/.test(trimmed)) {
        return LLM_DEFAULT_MAX_TOKENS;
    }
    const value = Number(trimmed);
    return Number.isSafeInteger(value) && value > 0 ? value : LLM_DEFAULT_MAX_TOKENS;
}

/** streamChatCompletion 的可选参数 */
export interface StreamChatOptions {
    /** 对话消息列表 */
    messages: ChatMessage[];
    /** 外部中止信号；触发后抛出 kind 为 'aborted' 的 LlmError */
    signal?: AbortSignal;
    /** 每个正文增量回调：第一个参数为累计全文，第二个参数为本帧增量 */
    onDelta?: (fullText: string, delta: string) => void;
    /**
     * 每个思维链增量回调（思考模型的 reasoning_content）：第一个参数为累计思维链全文，
     * 第二个参数为本帧增量。非思考模型不触发；可用于区分“等待首字节”与“模型正在思考”
     */
    onReasoning?: (fullReasoning: string, delta: string) => void;
    /** true 时请求体加 response_format: { type: 'json_object' }，强制模型输出 JSON */
    jsonMode?: boolean;
    /** 采样温度，默认 0.2 */
    temperature?: number;
    /**
     * 首字节超时（毫秒）：发出请求到收到响应头的最长等待，默认 LLM_FIRST_BYTE_TIMEOUT_MS。
     * 以下三种超时任一触发都抛出 kind 为 'timeout' 的 LlmError（message 注明是哪一种）
     */
    firstByteTimeoutMs?: number;
    /**
     * 流空闲超时（毫秒）：收到响应头后，相邻两次收到数据的最长间隔，每收到一个 chunk 重置，
     * 默认 LLM_IDLE_TIMEOUT_MS。思考模型持续输出思维链时不会触发
     */
    idleTimeoutMs?: number;
    /** 整个请求（含读完流）的宽松总上限（毫秒），默认 LLM_TOTAL_TIMEOUT_MS */
    totalTimeoutMs?: number;
}

/**
 * 流式请求的默认超时。不能用单一的总超时：思考模型的思维链常常超过 20 秒，
 * 总超时会把已经输出了一半的结果整体作废。因此拆成两段：
 * - 首字节：服务不可达、请求卡住时尽快失败（本地模型首次加载可能要十几秒，留足余量）；
 * - 流空闲：只要还在出数据就继续等；OpenAI o 系列等不流式输出思维链的模型，
 *   思考期间可能长时间没有数据，所以放宽到 60 秒；
 * - 总上限：宽松兜底，防止服务端无限慢速输出；用户也可以随时点“停止”。
 */
export const LLM_FIRST_BYTE_TIMEOUT_MS = 30_000;
export const LLM_IDLE_TIMEOUT_MS = 60_000;
export const LLM_TOTAL_TIMEOUT_MS = 180_000;

/** LLM 错误类别，调用方据此决定后续处理（如 aborted 静默忽略） */
export type LlmErrorKind = 'aborted' | 'timeout' | 'http' | 'network' | 'bad_response';

/** 各错误类别的默认提示；构造时显式传入 message 则以传入值为准 */
const LLM_ERROR_DEFAULT_MESSAGES: Record<LlmErrorKind, string> = {
    aborted: 'LLM request was aborted',
    timeout: 'LLM request timed out',
    http: 'LLM request returned a non-2xx HTTP status',
    network: 'LLM request failed at the network layer',
    bad_response: 'LLM response was empty or malformed',
};

/** LLM 请求失败的统一错误类型 */
export class LlmError extends Error {
    /** 错误类别 */
    public readonly kind: LlmErrorKind;
    /** HTTP 状态码，仅 kind 为 'http' 时有值 */
    public readonly status?: number;

    public constructor(kind: LlmErrorKind, status?: number, message?: string) {
        super(message ?? LLM_ERROR_DEFAULT_MESSAGES[kind]);
        this.name = 'LlmError';
        this.kind = kind;
        this.status = status;
    }
}

/** baseUrl/apiKey/model 三者（trim 后）均非空才算就绪 */
export function isLlmReady(cfg: LlmRequestConfig): boolean {
    return cfg.baseUrl.trim().length > 0
        && cfg.apiKey.trim().length > 0
        && cfg.model.trim().length > 0;
}

/**
 * 归一化 baseUrl 并拼接 chat completions 端点。
 * - 先去掉首尾空白与末尾的所有 '/'
 * - 若 baseUrl 本身已以 /chat/completions 结尾则原样返回，不再重复拼接
 */
function buildChatCompletionsUrl(baseUrl: string): string {
    const root = baseUrl.trim().replace(/\/+$/, '');
    if (root.endsWith('/chat/completions')) {
        return root;
    }
    return `${root}/chat/completions`;
}

/**
 * 归一化 baseUrl 并拼接 models 列表端点（GET /models）。
 * 归一化规则与 buildChatCompletionsUrl 一致；若用户把完整对话端点粘进了 baseUrl
 * （以 /chat/completions 结尾），先剥掉该后缀再拼 /models，保证列表与对话打向同一服务。
 */
function buildModelsUrl(baseUrl: string): string {
    let root = baseUrl.trim().replace(/\/+$/, '');
    if (root.endsWith('/chat/completions')) {
        root = root.slice(0, -'/chat/completions'.length).replace(/\/+$/, '');
    }
    if (root.endsWith('/models')) {
        return root;
    }
    return `${root}/models`;
}

// #region 请求参数兼容降级

/**
 * 请求参数的兼容降级项。不同服务/模型族对参数的接受程度不同，例如 OpenAI o 系列与
 * gpt-5 系列拒绝 max_tokens（要求 max_completion_tokens）、只接受默认 temperature；
 * 部分第三方服务不认 reasoning_effort 或 response_format。遇到 400/422 时逐项降级重试。
 */
type CompatStep = 'omit_temperature' | 'max_completion_tokens' | 'omit_response_format' | 'omit_reasoning_effort';

/** 推理强度 trim 并转小写；为空时返回 null（不传该字段） */
function normalizeReasoningEffort(cfg: LlmRequestConfig): string | null {
    // 部分服务端对取值大小写敏感（"High" 可能被拒），统一转小写
    const reasoningEffort = cfg.reasoningEffort?.trim().toLowerCase();
    return reasoningEffort != null && reasoningEffort.length > 0 ? reasoningEffort : null;
}

/**
 * 各降级项：hint 用于匹配上游错误信息（小写化后）中点名的参数；applicable 判断该项对
 * 本次请求是否有意义（没发送的字段无需降级）。数组顺序即“错误信息没点名任何参数”时的
 * 盲试顺序：reasoning_effort 是第三方服务最常见的不兼容字段，排最前。
 */
const COMPAT_STEPS: readonly {
    step: CompatStep;
    hint: RegExp;
    applicable: (cfg: LlmRequestConfig, opts: StreamChatOptions) => boolean;
}[] = [
    { step: 'omit_reasoning_effort', hint: /reasoning[_ ]?effort/, applicable: cfg => normalizeReasoningEffort(cfg) != null },
    { step: 'omit_temperature', hint: /temperature/, applicable: () => true },
    // OpenAI 的报错原文：“Unsupported parameter: 'max_tokens' ... Use 'max_completion_tokens' instead.”
    { step: 'max_completion_tokens', hint: /max_tokens/, applicable: () => true },
    { step: 'omit_response_format', hint: /response_format|json_object/, applicable: (_, opts) => opts.jsonMode === true },
];

/**
 * 成功请求所用的降级组合，按“端点 + 模型”记住，后续请求直接沿用，
 * 避免每次优选都先撞几次 400。只在请求成功时写入。
 */
const compatCache = new Map<string, ReadonlySet<CompatStep>>();

function compatCacheKey(cfg: LlmRequestConfig): string {
    return `${buildChatCompletionsUrl(cfg.baseUrl)}\n${cfg.model.trim()}`;
}

/**
 * 根据失败结果决定下一次重试的降级组合；不应重试时返回 null。
 * 仅 400/422 才降级。错误信息点名了尚未降级的参数时一次降级所有被点名的项，否则按顺序
 * 盲试一项。每次返回的组合都严格大于当前组合，降级项有限，所以重试次数有上限（至多 4 次）。
 */
function nextCompat(
    error: unknown,
    cfg: LlmRequestConfig,
    opts: StreamChatOptions,
    current: ReadonlySet<CompatStep>
): Set<CompatStep> | null {
    if (!(error instanceof LlmError) || error.kind !== 'http' || (error.status !== 400 && error.status !== 422)) {
        return null;
    }
    const pending = COMPAT_STEPS.filter(item => !current.has(item.step) && item.applicable(cfg, opts));
    if (pending.length === 0) {
        return null;
    }
    const message = error.message.toLowerCase();
    const hinted = pending.filter(item => item.hint.test(message));
    const chosen = hinted.length > 0 ? hinted : pending.slice(0, 1);
    return new Set([...current, ...chosen.map(item => item.step)]);
}
// #endregion

/** 组装 OpenAI 兼容的请求体；compat 中的降级项决定省略/替换哪些字段 */
function buildRequestBody(
    cfg: LlmRequestConfig,
    opts: StreamChatOptions,
    compat: ReadonlySet<CompatStep>
): Record<string, unknown> {
    const body: Record<string, unknown> = {
        model: cfg.model,
        messages: opts.messages,
        stream: true,
    };
    if (!compat.has('omit_temperature')) {
        body.temperature = opts.temperature ?? 0.2;
    }
    if (compat.has('max_completion_tokens')) {
        body.max_completion_tokens = cfg.maxTokens;
    } else {
        body.max_tokens = cfg.maxTokens;
    }
    if (opts.jsonMode === true && !compat.has('omit_response_format')) {
        body.response_format = { type: 'json_object' };
    }
    // 推理强度为空/空白时整体省略该字段，交由模型使用其默认值
    const reasoningEffort = normalizeReasoningEffort(cfg);
    if (reasoningEffort != null && !compat.has('omit_reasoning_effort')) {
        body.reasoning_effort = reasoningEffort;
    }
    return body;
}

/** SSE 单行的解析结果 */
type SseLine =
    // 携带增量的 data 行：正文（content）与思维链（reasoning）任一存在即有效，可能同时存在
    | { type: 'content'; content: string | null; reasoning: string | null }
    | { type: 'done' } // data: [DONE]，流式输出结束
    | { type: 'skip' }; // 空行/注释/event 行/解析失败的行

/** 解析单行 SSE；本函数不抛出异常（解析失败的行返回 skip） */
function parseSseLine(line: string): SseLine {
    // SSE 行可能以 \r\n 结尾，按 \n 切分后去掉行尾的 \r
    const text = line.endsWith('\r') ? line.slice(0, -1) : line;
    if (!text.startsWith('data:')) {
        return { type: 'skip' }; // 空行、注释（: 开头）、event:/id: 等行都忽略
    }
    const payload = text.slice('data:'.length).trim();
    if (payload === '[DONE]') {
        return { type: 'done' };
    }
    let chunk: unknown;
    try {
        chunk = JSON.parse(payload);
    } catch {
        return { type: 'skip' }; // 单行 JSON 解析失败不中断整个流
    }
    const { content, reasoning } = extractDelta(chunk);
    return content != null || reasoning != null
        ? { type: 'content', content, reasoning }
        : { type: 'skip' };
}

/**
 * 从 OpenAI 流式 chunk 中取出 choices[0].delta 的正文（content）与思维链（reasoning_content）增量。
 * 思维链常见于思考模型（如 DeepSeek V4、OpenAI o 系列），先于正文输出且通常远长于正文；
 * 对缺失/异常字段宽容，取不到时对应字段为 null。
 */
function extractDelta(chunk: unknown): { content: string | null; reasoning: string | null } {
    if (chunk == null || typeof chunk !== 'object') {
        return { content: null, reasoning: null };
    }
    const choices = (chunk as { choices?: unknown }).choices;
    if (!Array.isArray(choices) || choices.length === 0) {
        return { content: null, reasoning: null };
    }
    const first = choices[0] as { delta?: unknown } | null;
    if (first == null || typeof first !== 'object') {
        return { content: null, reasoning: null };
    }
    const delta = first.delta;
    if (delta == null || typeof delta !== 'object') {
        return { content: null, reasoning: null };
    }
    const content = (delta as { content?: unknown }).content;
    const reasoning = (delta as { reasoning_content?: unknown }).reasoning_content;
    return {
        content: typeof content === 'string' ? content : null,
        reasoning: typeof reasoning === 'string' ? reasoning : null,
    };
}

/**
 * 逐行读取 SSE 流并分别累加正文与思维链增量。
 * body 为空/无 reader 时抛出 kind 为 'bad_response' 的 LlmError。
 * onChunk 在每收到一块原始数据时调用（含注释/心跳行），供调用方重置流空闲计时。
 */
async function readSseStream(
    response: Response,
    onChunk: () => void,
    onDelta?: (fullText: string, delta: string) => void,
    onReasoning?: (fullReasoning: string, delta: string) => void
): Promise<{ content: string; reasoning: string }> {
    const body = response.body;
    if (body == null) {
        throw new LlmError('bad_response', undefined, 'response has no readable body');
    }
    const reader = body.getReader();
    // 用 { stream: true } 增量解码，正确处理跨 chunk 拆分的多字节字符（如中文/emoji）
    const decoder = new TextDecoder();
    let fullText = '';
    let fullReasoning = '';
    let buffer = ''; // 尚未遇到换行符的残留文本
    let receivedDone = false;
    /** 累加一条 data 行中的正文/思维链增量并触发对应回调 */
    const accumulate = (line: { content: string | null; reasoning: string | null }): void => {
        if (line.reasoning != null) {
            fullReasoning += line.reasoning;
            onReasoning?.(fullReasoning, line.reasoning);
        }
        if (line.content != null) {
            fullText += line.content;
            onDelta?.(fullText, line.content);
        }
    };
    try {
        while (!receivedDone) {
            const { done, value } = await reader.read();
            if (done) {
                break; // 对端关闭了流
            }
            onChunk();
            buffer += decoder.decode(value, { stream: true });
            // 按行拆分；最后一个不完整的行留在 buffer 中，等下一帧再处理
            let newlineIndex = buffer.indexOf('\n');
            while (newlineIndex !== -1) {
                const line = buffer.slice(0, newlineIndex);
                buffer = buffer.slice(newlineIndex + 1);
                const parsed = parseSseLine(line);
                if (parsed.type === 'done') {
                    receivedDone = true;
                    break;
                }
                if (parsed.type === 'content') {
                    accumulate(parsed);
                }
                newlineIndex = buffer.indexOf('\n');
            }
        }
        // 流结束时处理 buffer 中残留的、无换行结尾的最后一行
        if (!receivedDone) {
            buffer += decoder.decode(); // 冲刷多字节字符可能残留的字节
            const parsed = parseSseLine(buffer);
            if (parsed.type === 'content') {
                accumulate(parsed);
            }
        }
    } finally {
        // 提前退出（[DONE]/异常）时取消底层流以释放连接；流已读完时 cancel 为空操作
        reader.cancel().catch(() => { });
    }
    return { content: fullText, reasoning: fullReasoning };
}

/**
 * 发起 OpenAI 兼容的流式 chat completions 请求，resolve 完整的 content。
 *
 * 遇到 400/422 时按上游错误信息逐项降级请求参数后重试（见 COMPAT_STEPS），
 * 成功所用的降级组合按“端点 + 模型”缓存，后续请求直接沿用。
 *
 * 失败时抛出 LlmError：
 * - 'aborted'：外部 opts.signal 触发中止
 * - 'timeout'：首字节/流空闲/总上限任一超时（message 注明是哪一种）
 * - 'http'：非 2xx 响应（message 含截断的响应文本，status 含状态码）；降级重试用尽时为最后一次的错误
 * - 'network'：网络层失败
 * - 'bad_response'：body 为空/无 reader/流结束但 content 为空
 *   （若收到过思维链则 message 注明，提示思维链可能耗尽了 max_tokens 预算）
 */
export async function streamChatCompletion(cfg: LlmRequestConfig, opts: StreamChatOptions): Promise<string> {
    const cacheKey = compatCacheKey(cfg);
    let compat: ReadonlySet<CompatStep> = compatCache.get(cacheKey) ?? new Set<CompatStep>();
    for (;;) {
        try {
            const content = await streamChatCompletionOnce(cfg, opts, compat);
            compatCache.set(cacheKey, compat);
            return content;
        } catch (error) {
            const next = nextCompat(error, cfg, opts, compat);
            if (next == null) {
                throw error;
            }
            console.warn(`[llm] 请求参数不被接受，降级后重试（${[...next].join(', ')}）:`, error);
            compat = next;
        }
    }
}

/** 按给定降级组合发起一次流式请求（不含重试） */
async function streamChatCompletionOnce(
    cfg: LlmRequestConfig,
    opts: StreamChatOptions,
    compat: ReadonlySet<CompatStep>
): Promise<string> {
    const firstByteTimeoutMs = opts.firstByteTimeoutMs ?? LLM_FIRST_BYTE_TIMEOUT_MS;
    const idleTimeoutMs = opts.idleTimeoutMs ?? LLM_IDLE_TIMEOUT_MS;
    const totalTimeoutMs = opts.totalTimeoutMs ?? LLM_TOTAL_TIMEOUT_MS;

    // 用一个内部 AbortController 统一收口“超时”与“外部 signal”两路中止：
    // 任一路触发都调用 controller.abort()，使进行中的 fetch/reader reject，
    // 再在 catch 中依据触发源（timeoutMessage 是否非空）归类为 timeout 或 aborted
    const controller = new AbortController();
    let timeoutMessage = null as string | null;
    const fireTimeout = (message: string): void => {
        if (!controller.signal.aborted) { // 已被外部中止时不再改判为超时
            timeoutMessage = message;
            controller.abort();
        }
    };

    const externalSignal = opts.signal;
    const onExternalAbort = () => controller.abort();
    if (externalSignal != null) {
        if (externalSignal.aborted) {
            throw new LlmError('aborted'); // 外部信号已中止，不发起请求
        }
        externalSignal.addEventListener('abort', onExternalAbort);
    }

    // 阶段计时器：收到响应头之前是首字节超时，之后切换为流空闲超时（每收到数据重置）
    let phaseTimerId = window.setTimeout(() => {
        fireTimeout(`LLM request timed out: no response within ${firstByteTimeoutMs / 1000}s`);
    }, firstByteTimeoutMs);
    const resetIdleTimer = (): void => {
        clearTimeout(phaseTimerId);
        phaseTimerId = window.setTimeout(() => {
            fireTimeout(`LLM stream stalled: no data received for ${idleTimeoutMs / 1000}s`);
        }, idleTimeoutMs);
    };
    const totalTimerId = window.setTimeout(() => {
        fireTimeout(`LLM request exceeded the overall limit of ${totalTimeoutMs / 1000}s`);
    }, totalTimeoutMs);

    try {
        const response = await fetch(buildChatCompletionsUrl(cfg.baseUrl), {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Accept': 'text/event-stream',
                'Authorization': `Bearer ${cfg.apiKey}`,
            },
            body: JSON.stringify(buildRequestBody(cfg, opts, compat)),
            signal: controller.signal,
        });
        resetIdleTimer(); // 响应头已到达：切换到流空闲计时
        if (!response.ok) {
            // 读取响应文本并截取前 300 字符放进 message，便于定位鉴权/参数等错误
            const text = await response.text();
            throw new LlmError('http', response.status, `HTTP ${response.status}: ${text.slice(0, 300)}`);
        }
        const { content, reasoning } = await readSseStream(response, resetIdleTimer, opts.onDelta, opts.onReasoning);
        if (content.length === 0) {
            // 收到过思维链但正文为空：思考模型几乎必然是思维链耗尽了 max_tokens 预算，
            // 与“流中什么都没有”区分开，便于用户定位（调大最大生成 Token 数或调低思考强度）
            throw new LlmError('bad_response', undefined, reasoning.length > 0
                ? `stream ended with empty content after ${reasoning.length} chars of reasoning `
                    + '(thinking probably exhausted max_tokens; increase it or lower reasoning effort)'
                : 'stream ended with empty content');
        }
        return content;
    } catch (error) {
        if (controller.signal.aborted) {
            // 由我们主动 abort 引起的 reject（含读取错误响应体时被中止）：按触发源归类，不计为 network
            throw timeoutMessage != null ? new LlmError('timeout', undefined, timeoutMessage) : new LlmError('aborted');
        }
        if (error instanceof LlmError) {
            throw error; // 已归一化的业务错误（http/bad_response）直接透传
        }
        // 其余为真正的网络层失败（DNS、连接拒绝、TLS 等）
        const detail = error instanceof Error ? error.message : String(error);
        throw new LlmError('network', undefined, detail);
    } finally {
        // 函数返回前务必清除超时定时器
        clearTimeout(phaseTimerId);
        clearTimeout(totalTimerId);
        externalSignal?.removeEventListener('abort', onExternalAbort);
    }
}

// #region 模型列表拉取与连接测试（设置页辅助能力，与流式对话共用归一化/鉴权/错误体系）

/** fetchAvailableModels 的超时上限（毫秒） */
const FETCH_MODELS_TIMEOUT_MS = 10000;

/** fetchAvailableModels 返回的单个远端模型信息 */
export interface RemoteModelInfo {
    /** 模型 id（/models 响应中 data[].id，即请求体 model 字段应填的值） */
    id: string;
    /** 模型归属方（data[].owned_by，部分服务不返回该字段） */
    ownedBy?: string;
}

/**
 * 启发式判断模型 id 是否“疑似非对话模型”。
 * 聚合站（one-api/new-api 等）的 /models 会混入 embedding、语音、绘图等无法用于 chat
 * completions 的模型，按常见命名关键词排除。仅用于模型列表的默认过滤（UI 保留“显示全部”
 * 开关），启发式不保证准确，不能作为模型可用性的判定依据。
 */
export function isLikelyNonChatModel(id: string): boolean {
    return NON_CHAT_MODEL_PATTERN.test(id);
}

/** 疑似非对话模型的命名关键词（大小写不敏感的子串匹配） */
const NON_CHAT_MODEL_PATTERN = new RegExp([
    'embedding', 'embed', 'bge-', 'e5-', 'gte-',                          // 向量模型
    'rerank',                                                             // 重排模型
    'whisper', 'tts', 'speech', 'audio', 'transcri',                      // 语音模型
    'dall-e', 'dalle', 'stable-diffusion', 'sdxl', 'sd3', 'flux', 'midjourney',
    'image', 'cogview', 'seedream',                                       // 绘图模型
    'moderation', 'guard',                                                // 内容审核
    'clip',                                                               // 多模态向量
].join('|'), 'i');

/**
 * 非流式请求的统一传输封装（供 fetchAvailableModels 使用）：
 * 合并“超时”与“外部 signal”两路中止，超时/中止/网络层失败分别抛出对应 kind 的 LlmError。
 * 读取响应体也在同一 controller 与计时范围内：由 consume 读完并解析后才清理，
 * 否则服务端发完响应头后慢速传输响应体时，超时与外部取消都不再生效。
 */
async function fetchWithTimeout<T>(
    url: string,
    init: RequestInit,
    timeoutMs: number,
    externalSignal: AbortSignal | undefined,
    consume: (response: Response) => Promise<T>
): Promise<T> {
    const controller = new AbortController();
    let timedOut = false;
    const onExternalAbort = () => controller.abort();
    if (externalSignal != null) {
        if (externalSignal.aborted) {
            throw new LlmError('aborted');
        }
        externalSignal.addEventListener('abort', onExternalAbort);
    }
    const timeoutId = window.setTimeout(() => {
        timedOut = true;
        controller.abort();
    }, timeoutMs);
    try {
        const response = await fetch(url, { ...init, signal: controller.signal });
        return await consume(response);
    } catch (error) {
        // 先看是否由中止引起：consume 读 body 被中止时可能已把异常包装成 bad_response
        if (controller.signal.aborted) {
            throw timedOut ? new LlmError('timeout') : new LlmError('aborted');
        }
        if (error instanceof LlmError) {
            throw error;
        }
        const detail = error instanceof Error ? error.message : String(error);
        throw new LlmError('network', undefined, detail);
    } finally {
        clearTimeout(timeoutId);
        externalSignal?.removeEventListener('abort', onExternalAbort);
    }
}

/**
 * 拉取 OpenAI 兼容服务的可用模型列表（GET /models，Bearer 鉴权，10s 超时）。
 *
 * 解析对非标准服务宽容：标准响应为 { data: [...] }，个别服务直接返回顶层数组；
 * 忽略 id 缺失/非字符串的条目，按 id 去重并排序（数字感知、大小写不敏感）。
 * 返回空数组表示服务成功响应但未给出任何模型。
 *
 * 失败时抛出 LlmError（kind 与 streamChatCompletion 一致；'http' 的 status 可用于
 * 区分 401 鉴权失败与 404 端点未实现）。
 */
export async function fetchAvailableModels(
    cfg: Pick<LlmRequestConfig, 'baseUrl' | 'apiKey'>,
    signal?: AbortSignal
): Promise<RemoteModelInfo[]> {
    const payload = await fetchWithTimeout(buildModelsUrl(cfg.baseUrl), {
        method: 'GET',
        headers: {
            'Accept': 'application/json',
            'Authorization': `Bearer ${cfg.apiKey}`,
        },
    }, FETCH_MODELS_TIMEOUT_MS, signal, async (response): Promise<unknown> => {
        if (!response.ok) {
            const text = await response.text();
            throw new LlmError('http', response.status, `HTTP ${response.status}: ${text.slice(0, 300)}`);
        }
        try {
            return await response.json();
        } catch {
            throw new LlmError('bad_response', undefined, 'models response is not valid JSON');
        }
    });
    const items = Array.isArray(payload)
        ? payload
        : Array.isArray((payload as { data?: unknown } | null)?.data)
            ? (payload as { data: unknown[] }).data
            : [];
    // Map 按 id 去重（部分聚合站会返回重复条目），保留首次出现的 owned_by
    const models = new Map<string, RemoteModelInfo>();
    for (const item of items) {
        if (item == null || typeof item !== 'object') {
            continue;
        }
        const id = (item as { id?: unknown }).id;
        if (typeof id !== 'string' || id.trim().length === 0 || models.has(id)) {
            continue;
        }
        const ownedBy = (item as { owned_by?: unknown }).owned_by;
        models.set(id, {
            id,
            ownedBy: typeof ownedBy === 'string' && ownedBy.length > 0 ? ownedBy : undefined,
        });
    }
    return [...models.values()].sort((a, b) =>
        a.id.localeCompare(b.id, undefined, { sensitivity: 'base', numeric: true }));
}

/**
 * 发送一条极小的补全请求以验证 API 地址、Key 与模型真实可用，返回耗时（毫秒）。
 *
 * 与 AI 优选走完全相同的路径：同为 streamChatCompletion，同样带 temperature、max_tokens、
 * response_format（json_object）与 reasoning_effort，同样的超时与 400/422 参数降级，
 * 降级结果也写入同一缓存。这样“测试通过”即意味着真实请求的参数也被接受，
 * 不会再出现测试通过、实际优选失败的情况。
 * 调用方应传入与真实请求一致的 maxTokens/reasoningEffort；缺省时 maxTokens 取默认值、不传推理强度。
 * 非 2xx 抛 'http'（message 含上游错误原文，便于定位 key/额度/模型名问题）。
 */
export async function testLlmConnection(
    cfg: Pick<LlmRequestConfig, 'baseUrl' | 'apiKey' | 'model'> & Partial<Pick<LlmRequestConfig, 'maxTokens' | 'reasoningEffort'>>,
    signal?: AbortSignal
): Promise<number> {
    const startedAt = performance.now();
    await streamChatCompletion({
        baseUrl: cfg.baseUrl,
        apiKey: cfg.apiKey,
        model: cfg.model,
        maxTokens: cfg.maxTokens ?? LLM_DEFAULT_MAX_TOKENS,
        reasoningEffort: cfg.reasoningEffort,
    }, {
        // OpenAI 的 json_object 模式要求消息中出现 “JSON” 字样，否则直接 400
        messages: [{ role: 'user', content: 'Reply with the JSON object {"ok":true} and nothing else.' }],
        signal,
        jsonMode: true,
    });
    return Math.round(performance.now() - startedAt);
}
// #endregion
