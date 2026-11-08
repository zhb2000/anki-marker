/**
 * LLM / OpenAI 兼容接口的 dev 夹具。
 *
 * 放在前端 HTTP 入口（src/logics/net.ts）之后，因此原生 dev 窗口、dev:ai 浏览器页、
 * dev:browser 纯夹具三种环境共用同一套行为，不依赖 Tauri 的 plugin-http 打桩。
 *
 * 夹具的基础场景是“命中候选”：从请求体的结构化输出 schema 里取出真实候选编号返回，
 * 保证 UI 能走到 pick 分支。实际场景由 effectiveLlmScenario 解析（auto 在浏览器下即 pick，
 * 在 Tauri 下为 real）；只有实际场景为 'real' 时才交回真实网络栈。
 */
import { effectiveLlmScenario, type EffectiveLlmScenario } from '../effective';
import { currentDevEnvironment } from '../environment';
import { devSettings } from '../state';

/** 模拟 AI 连接配置：只在 mockLlmConfig 打开且场景不是真实请求时注入 read_config */
export const MOCK_LLM_BASE_URL = 'http://dev-mock.local/v1';
export const MOCK_LLM_API_KEY = 'dev-mock-key';
export const MOCK_LLM_MODEL = 'dev-mock-model';

/** 需要夹具处理的请求：chat completions 与模型列表 */
export function isLlmMockUrl(url: URL, method: string): boolean {
    if (effectiveLlmScenario(devSettings, currentDevEnvironment()) === 'real') {
        return false;
    }
    return (method === 'POST' && url.pathname.endsWith('/chat/completions'))
        || (method === 'GET' && url.pathname.endsWith('/models'));
}

export interface LlmMockRequest {
    url: URL;
    method: string;
    body: unknown;
    signal?: AbortSignal | null;
    /** 响应流结束（或被取消/中止）时调用，用于 idle() 的活动计数 */
    onDone: () => void;
}

type JsonObject = Record<string, unknown>;

function asObject(value: unknown): JsonObject | null {
    return value != null && typeof value === 'object' && !Array.isArray(value) ? value as JsonObject : null;
}

function field(object: unknown, key: string): unknown {
    const obj = asObject(object);
    return obj == null ? undefined : obj[key];
}

/** 从请求体的 response_format 中取出本次允许的候选编号 */
function candidateIdsFromSchema(body: unknown): string[] {
    const pick = field(field(field(field(body, 'response_format'), 'json_schema'), 'schema'), 'properties');
    const pickField = field(pick, 'pick');
    const direct = field(pickField, 'enum');
    if (Array.isArray(direct)) {
        return direct.filter((item): item is string => typeof item === 'string');
    }
    const anyOf = field(pickField, 'anyOf');
    if (Array.isArray(anyOf)) {
        for (const option of anyOf) {
            const values = field(option, 'enum');
            if (Array.isArray(values)) {
                return values.filter((item): item is string => typeof item === 'string');
            }
        }
    }
    return [];
}

/**
 * 参数降级后 response_format 会被去掉，schema 里的候选编号也随之消失；
 * 从用户消息的候选行（如 “[co3f9] collins: …”）里兜底提取，保证兼容降级场景仍能命中候选。
 */
function candidateIdsFromMessages(body: unknown): string[] {
    const messages = field(body, 'messages');
    if (!Array.isArray(messages)) {
        return [];
    }
    // 只看最后一条 user 消息：system prompt 里也有 “[co3f9]” 这种格式示例，不能当成真实候选
    const userTexts = messages
        .filter(message => field(message, 'role') === 'user')
        .map(message => field(message, 'content'))
        .filter((content): content is string => typeof content === 'string');
    const text = userTexts[userTexts.length - 1] ?? '';
    const candidateStart = text.indexOf('候选义项：');
    const candidateText = candidateStart >= 0 ? text.slice(candidateStart) : text;
    const ids = new Set<string>();
    const pattern = /\[([a-z]{2}[a-z0-9]+)\]/g;
    for (const match of candidateText.matchAll(pattern)) {
        if (match[1] != null) {
            ids.add(match[1]);
        }
    }
    return [...ids];
}

/** 本次允许的候选编号：优先 schema，缺失时从候选行提取 */
function candidateIds(body: unknown): string[] {
    const fromSchema = candidateIdsFromSchema(body);
    return fromSchema.length > 0 ? fromSchema : candidateIdsFromMessages(body);
}

/** 本次请求是否是设置页的“测试连接”（schema 名称为 ping） */
function isPingRequest(body: unknown): boolean {
    return field(field(field(body, 'response_format'), 'json_schema'), 'name') === 'ping';
}

/** 请求消息里是否带有 AI 优选的“编号纠正”提醒（用于 bad-pick-retry 的第二次请求） */
function hasPickNudge(body: unknown): boolean {
    const messages = field(body, 'messages');
    if (!Array.isArray(messages)) {
        return false;
    }
    return messages.some(message => {
        const content = field(message, 'content');
        return typeof content === 'string' && content.includes('pick 必须逐字复制');
    });
}

function validPickBody(body: unknown): string {
    const ids = candidateIds(body);
    if (ids.length === 0) {
        return JSON.stringify({
            kind: 'word', pick: null,
            contextual_def: '（dev mock）没有候选时的语境释义',
            note: '（dev mock）候选为空，走 AI 生成分支',
        });
    }
    return JSON.stringify({
        kind: 'word', pick: ids[0],
        contextual_def: '',
        note: '（dev mock）已选中第一条候选',
    });
}

/** 按场景生成最终正文（null 表示空响应） */
function contentFor(scenario: EffectiveLlmScenario, body: unknown): string | null {
    if (isPingRequest(body)) {
        return JSON.stringify({ ok: true });
    }
    switch (scenario) {
        case 'pick':
        case 'thinking':
        case 'compat-retry':
            return validPickBody(body);
        case 'bad-pick-retry':
            return hasPickNudge(body)
                ? validPickBody(body)
                : JSON.stringify({
                    kind: 'word', pick: 'co00000',
                    contextual_def: '（dev mock）第一次故意给出不存在的编号',
                    note: '（dev mock）等待纠正重试',
                });
        case 'fallback':
            return JSON.stringify({
                kind: 'word', pick: null,
                contextual_def: '（dev mock）按当前语境生成的释义',
                note: '（dev mock）没有合适候选，走 AI 生成',
            });
        case 'sentence':
            return JSON.stringify({
                kind: 'sentence', pick: null,
                contextual_def: '（dev mock）这是一句用于预览整句翻译卡片的模拟译文。',
                note: '',
            });
        case 'misspelling':
            return JSON.stringify({
                kind: 'misspelling', pick: null,
                contextual_def: '',
                note: '（dev mock）正确拼写：design',
            });
        case 'gibberish':
            return JSON.stringify({
                kind: 'gibberish', pick: null,
                contextual_def: '',
                note: '（dev mock）无法识别的乱码或误触',
            });
        case 'bad-json':
            return '{"kind":"word","pick":';
        case 'empty':
        case 'http-error':
            return null;
        default:
            return validPickBody(body);
    }
}

function jsonResponse(value: unknown, status = 200): Response {
    return new Response(JSON.stringify(value), {
        status,
        headers: { 'content-type': 'application/json' },
    });
}

function textResponse(text: string, status: number): Response {
    return new Response(text, {
        status,
        headers: { 'content-type': 'text/plain; charset=utf-8' },
    });
}

/** 把一段文本按码点切成小块，避免拆开 surrogate pair */
function splitByCodePoint(text: string, size: number): string[] {
    const chars = Array.from(text);
    const chunks: string[] = [];
    for (let index = 0; index < chars.length; index += size) {
        chunks.push(chars.slice(index, index + size).join(''));
    }
    return chunks;
}

/** 生成一条 SSE delta */
function deltaEvent(kind: 'content' | 'reasoning', text: string): unknown {
    return {
        choices: [{
            delta: kind === 'content' ? { content: text } : { reasoning_content: text },
        }],
    };
}

/**
 * 按块推送 SSE。延迟来自 llmStreamDelayMs：0 时仍会逐块 setTimeout(0)，
 * 让 thinking / streaming 至少有一次渲染机会；中止信号会立即结束流。
 */
function sseResponse(events: unknown[], delayMs: number, signal: AbortSignal | null | undefined, onDone: () => void): Response {
    const encoder = new TextEncoder();
    let timer: number | null = null;
    let finished = false;
    let abortHandler: (() => void) | null = null;

    const finish = (): void => {
        if (finished) {
            return;
        }
        finished = true;
        if (timer != null) {
            window.clearTimeout(timer);
            timer = null;
        }
        if (abortHandler != null) {
            signal?.removeEventListener('abort', abortHandler);
        }
        onDone();
    };

    const stream = new ReadableStream<Uint8Array>({
        start(controller) {
            const fail = (): void => {
                try {
                    controller.error(new DOMException('aborted', 'AbortError'));
                } catch {
                    // 流已经结束
                }
                finish();
            };
            if (signal?.aborted === true) {
                fail();
                return;
            }
            abortHandler = fail;
            signal?.addEventListener('abort', fail, { once: true });

            let index = 0;
            const tick = (): void => {
                if (signal?.aborted === true) {
                    finish();
                    return;
                }
                if (index >= events.length) {
                    controller.enqueue(encoder.encode('data: [DONE]\n\n'));
                    try {
                        controller.close();
                    } catch {
                        // 已被取消
                    }
                    finish();
                    return;
                }
                controller.enqueue(encoder.encode(`data: ${JSON.stringify(events[index])}\n\n`));
                index += 1;
                timer = window.setTimeout(tick, Math.max(0, delayMs));
            };
            tick();
        },
        cancel() {
            finish();
        },
    });

    return new Response(stream, {
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
    });
}

/** 模型列表：混入常见的非对话模型，便于检查“显示全部”与过滤逻辑 */
function modelsResponse(): Response {
    return jsonResponse({
        data: [
            { id: 'mock-model-a', owned_by: 'dev' },
            { id: 'mock-model-b', owned_by: 'dev' },
            { id: 'mock-chat-large', owned_by: 'dev' },
            { id: 'text-embedding-3-small', owned_by: 'dev' },
            { id: 'whisper-1', owned_by: 'dev' },
            { id: 'dall-e-3', owned_by: 'dev' },
        ],
    });
}

/** 处理一次 LLM 请求；调用方负责判断 isLlmMockUrl 并传入已解析的 body */
export function handleLlmMock(request: LlmMockRequest): Response {
    const { method, body, signal, onDone } = request;
    const scenario = effectiveLlmScenario(devSettings, currentDevEnvironment());

    if (method === 'GET') {
        if (scenario === 'http-error') {
            onDone();
            return textResponse('mock model list error', 500);
        }
        if (scenario === 'empty') {
            onDone();
            return jsonResponse({ data: [] });
        }
        onDone();
        return modelsResponse();
    }

    if (scenario === 'http-error') {
        onDone();
        return textResponse('mock chat error: invalid api key (dev mock)', 401);
    }

    // 参数兼容降级：只拒绝 strict json_schema，接受降级后的 json_object / 无 response_format
    const responseFormatType = field(field(body, 'response_format'), 'type');
    if (scenario === 'compat-retry' && responseFormatType === 'json_schema') {
        onDone();
        return textResponse(
            "Unsupported parameter: 'response_format' (dev mock). Use 'json_object' instead.",
            400,
        );
    }

    const content = contentFor(scenario, body);
    const events: unknown[] = [];
    if (scenario === 'thinking') {
        const reasoning = '（dev mock）先判断查询类型，再逐条比较词典候选的义项，最后给出结论。';
        for (const piece of splitByCodePoint(reasoning, 5)) {
            events.push(deltaEvent('reasoning', piece));
        }
    }
    if (content != null) {
        for (const piece of splitByCodePoint(content, 10)) {
            events.push(deltaEvent('content', piece));
        }
    }
    return sseResponse(events, devSettings.llmStreamDelayMs, signal, onDone);
}
