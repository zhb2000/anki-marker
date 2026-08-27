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
    /** 推理强度；非空时以 OpenAI 风格 reasoning_effort 字段透传，空/undefined 不传 */
    reasoningEffort?: string;
}

/** 单条聊天消息 */
export type ChatMessage = { role: 'system' | 'user' | 'assistant'; content: string };

/** streamChatCompletion 的可选参数 */
export interface StreamChatOptions {
    /** 对话消息列表 */
    messages: ChatMessage[];
    /** 外部中止信号；触发后抛出 kind 为 'aborted' 的 LlmError */
    signal?: AbortSignal;
    /** 每个增量回调：第一个参数为累计全文，第二个参数为本帧增量 */
    onDelta?: (fullText: string, delta: string) => void;
    /** true 时请求体加 response_format: { type: 'json_object' }，强制模型输出 JSON */
    jsonMode?: boolean;
    /** 采样温度，默认 0.2 */
    temperature?: number;
    /** 最大生成 token 数，默认 512 */
    maxTokens?: number;
    /** 超时时间（毫秒），默认 20000；超时抛出 kind 为 'timeout' 的 LlmError */
    timeoutMs?: number;
}

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

/** 组装 OpenAI 兼容的请求体 */
function buildRequestBody(cfg: LlmRequestConfig, opts: StreamChatOptions): Record<string, unknown> {
    const body: Record<string, unknown> = {
        model: cfg.model,
        messages: opts.messages,
        stream: true,
        temperature: opts.temperature ?? 0.2,
        max_tokens: opts.maxTokens ?? 512,
    };
    if (opts.jsonMode === true) {
        body.response_format = { type: 'json_object' };
    }
    // 推理强度为空/空白时整体省略该字段，交由模型使用其默认值
    const reasoningEffort = cfg.reasoningEffort?.trim();
    if (reasoningEffort != null && reasoningEffort.length > 0) {
        body.reasoning_effort = reasoningEffort;
    }
    return body;
}

/** SSE 单行的解析结果 */
type SseLine =
    | { type: 'content'; content: string } // 携带内容增量的 data 行
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
    const content = extractDeltaContent(chunk);
    return content != null ? { type: 'content', content } : { type: 'skip' };
}

/**
 * 从 OpenAI 流式 chunk 中取出 choices[0].delta.content。
 * 忽略 reasoning_content；对缺失/异常字段宽容，取不到时返回 null。
 */
function extractDeltaContent(chunk: unknown): string | null {
    if (chunk == null || typeof chunk !== 'object') {
        return null;
    }
    const choices = (chunk as { choices?: unknown }).choices;
    if (!Array.isArray(choices) || choices.length === 0) {
        return null;
    }
    const first = choices[0] as { delta?: unknown } | null;
    if (first == null || typeof first !== 'object') {
        return null;
    }
    const delta = first.delta;
    if (delta == null || typeof delta !== 'object') {
        return null;
    }
    const content = (delta as { content?: unknown }).content;
    return typeof content === 'string' ? content : null;
}

/**
 * 逐行读取 SSE 流并累加内容增量，返回完整 content。
 * body 为空/无 reader 时抛出 kind 为 'bad_response' 的 LlmError。
 */
async function readSseStream(
    response: Response,
    onDelta?: (fullText: string, delta: string) => void
): Promise<string> {
    const body = response.body;
    if (body == null) {
        throw new LlmError('bad_response', undefined, 'response has no readable body');
    }
    const reader = body.getReader();
    // 用 { stream: true } 增量解码，正确处理跨 chunk 拆分的多字节字符（如中文/emoji）
    const decoder = new TextDecoder();
    let fullText = '';
    let buffer = ''; // 尚未遇到换行符的残留文本
    let receivedDone = false;
    try {
        while (!receivedDone) {
            const { done, value } = await reader.read();
            if (done) {
                break; // 对端关闭了流
            }
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
                    fullText += parsed.content;
                    onDelta?.(fullText, parsed.content);
                }
                newlineIndex = buffer.indexOf('\n');
            }
        }
        // 流结束时处理 buffer 中残留的、无换行结尾的最后一行
        if (!receivedDone) {
            buffer += decoder.decode(); // 冲刷多字节字符可能残留的字节
            const parsed = parseSseLine(buffer);
            if (parsed.type === 'content') {
                fullText += parsed.content;
                onDelta?.(fullText, parsed.content);
            }
        }
    } finally {
        // 提前退出（[DONE]/异常）时取消底层流以释放连接；流已读完时 cancel 为空操作
        reader.cancel().catch(() => { });
    }
    return fullText;
}

/**
 * 发起 OpenAI 兼容的流式 chat completions 请求，resolve 完整的 content。
 *
 * 失败时抛出 LlmError：
 * - 'aborted'：外部 opts.signal 触发中止
 * - 'timeout'：超过 timeoutMs
 * - 'http'：非 2xx 响应（message 含截断的响应文本，status 含状态码）
 * - 'network'：网络层失败
 * - 'bad_response'：body 为空/无 reader/流结束但 content 为空
 */
export async function streamChatCompletion(cfg: LlmRequestConfig, opts: StreamChatOptions): Promise<string> {
    const timeoutMs = opts.timeoutMs ?? 20000;

    // 用一个内部 AbortController 统一收口“超时”与“外部 signal”两路中止：
    // 任一路触发都调用 controller.abort()，使进行中的 fetch/reader reject，
    // 再在 catch 中依据触发源（timedOut 标记）归类为 timeout 或 aborted
    const controller = new AbortController();
    let timedOut = false;

    const externalSignal = opts.signal;
    const onExternalAbort = () => controller.abort();
    if (externalSignal != null) {
        if (externalSignal.aborted) {
            throw new LlmError('aborted'); // 外部信号已中止，不发起请求
        }
        externalSignal.addEventListener('abort', onExternalAbort);
    }

    const timeoutId = window.setTimeout(() => {
        timedOut = true;
        controller.abort();
    }, timeoutMs);

    try {
        const response = await fetch(buildChatCompletionsUrl(cfg.baseUrl), {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Accept': 'text/event-stream',
                'Authorization': `Bearer ${cfg.apiKey}`,
            },
            body: JSON.stringify(buildRequestBody(cfg, opts)),
            signal: controller.signal,
        });
        if (!response.ok) {
            // 读取响应文本并截取前 300 字符放进 message，便于定位鉴权/参数等错误
            const text = await response.text();
            throw new LlmError('http', response.status, `HTTP ${response.status}: ${text.slice(0, 300)}`);
        }
        const fullText = await readSseStream(response, opts.onDelta);
        if (fullText.length === 0) {
            throw new LlmError('bad_response', undefined, 'stream ended with empty content');
        }
        return fullText;
    } catch (error) {
        if (error instanceof LlmError) {
            throw error; // 已归一化的业务错误（http/bad_response）直接透传
        }
        if (controller.signal.aborted) {
            // 由我们主动 abort 引起的 reject：按触发源归类，不计为 network
            throw timedOut ? new LlmError('timeout') : new LlmError('aborted');
        }
        // 其余为真正的网络层失败（DNS、连接拒绝、TLS 等）
        const detail = error instanceof Error ? error.message : String(error);
        throw new LlmError('network', undefined, detail);
    } finally {
        clearTimeout(timeoutId); // 函数返回前务必清除超时定时器
        externalSignal?.removeEventListener('abort', onExternalAbort);
    }
}
