import * as dict from './dict';
import { LlmError, streamChatCompletion, type ChatMessage, type LlmRequestConfig } from './llm';

/**
 * AI 优选释义的编排逻辑（纯逻辑模块，不依赖 Vue）。
 *
 * 流程：并行查询三本词典（或由调用方传入预取结果）→ 组装编号候选 →
 * 流式调用 LLM 做义项消歧 → 容错解析 JSON 结果 → 通过 onUpdate 推进状态。
 *
 * 结果按“句子|单词”做内存缓存，重复搜索直接命中。
 */

/** 词典来源 */
export type DictSource = 'collins' | 'oxford' | 'youdao';

/** AI 优选的状态机阶段 */
export type AiPickPhase = 'idle' | 'loading' | 'streaming' | 'done' | 'error';

/** AI 优选命中的词典条目 */
export interface AiPick {
    source: DictSource;
    /** 该词典候选列表（与界面条目列表一致）中的索引 */
    index: number;
}

/** AI 优选卡片的完整状态 */
export interface AiPickState {
    phase: AiPickPhase;
    /** 命中的候选；fallback 或尚未解析完成时为 null */
    pick: AiPick | null;
    /** 目标词在此句语境下的简明中文释义（流式期间为增量部分值） */
    contextualDef: string;
    /** 搭配/用法/易混提示，无则空串 */
    note: string;
    /** true 表示释义由 AI 凭自身知识生成（无候选或未选中任何候选） */
    fallback: boolean;
}

/** 三本词典的候选来源条目 */
export interface AiPickSources {
    collins: dict.CollinsItem[];
    oxford: dict.OxfordItem[];
    youdao: dict.YoudaoItem[];
}

/** 解析后的 AI 结果（已校验） */
export interface ParsedAiPick {
    pick: AiPick | null;
    contextualDef: string;
    note: string;
    fallback: boolean;
}

export interface RequestAiPickOptions {
    /** 目标词所在的句子 */
    sentence: string;
    /** 目标词 */
    word: string;
    /** LLM 连接配置（调用方需先通过 isLlmReady 确认就绪） */
    config: LlmRequestConfig;
    /** 外部中止信号；'aborted' 错误静默处理（不回调 error 态） */
    signal?: AbortSignal;
    /**
     * 预取的三本词典结果（通常来自界面已有的搜索结果），保证候选编号与界面条目索引一致；
     * 缺省时内部并行查询三本词典（单本失败不拖垮整体，全部失败视为无候选）
     */
    sources?: Partial<AiPickSources>;
    /** 状态推进回调；每次回调传入一份完整的状态快照 */
    onUpdate: (state: AiPickState) => void;
}

/** 每本词典最多送入 LLM 的候选条数（控制 token 消耗）；有道条目短而多，上限放宽 */
const MAX_CANDIDATES: Record<DictSource, number> = { collins: 8, oxford: 8, youdao: 12 };
/** 内存缓存的最大条目数，超出时逐出最旧条目 */
const CACHE_MAX_ENTRIES = 200;

const SYSTEM_PROMPT = `你是词典义项消歧助手。给定英文句子、句中的目标词，以及来自多部词典的编号候选义项，选出最契合该语境的一项，并给出语境化释义。

只输出 JSON，不要输出其他任何文字：
{"pick":{"source":"collins"|"oxford"|"youdao"|null,"index":number|null},"contextual_def":"...","note":"...","fallback":boolean}

要求：
- pick.source 与 pick.index 共同指向一条候选（如候选行为 "collins#2"，则 source="collins"、index=2）；认为所有候选都不合适时二者均为 null。
- contextual_def：目标词在此句语境下的简明中文释义，不超过 20 字。
- note：一句搭配、用法或易混提示，不超过 30 字；没有合适内容时给空字符串 ""。
- 候选列表为空时：必须 pick=null、fallback=true，并凭自身知识生成 contextual_def；从候选中选出义项时 fallback=false。
- 候选列表非空时，只要存在基本契合语境的候选就必须从中选择（pick 指向该候选、fallback=false）；仅当候选为空，或全部候选都明显与语境无关时，才允许 fallback=true 并凭自身知识生成。`;

/** 结果内存缓存（仅缓存 done 态），key 见 makeAiPickCacheKey */
const resultCache = new Map<string, AiPickState>();

/** 生成一份初始（idle）状态 */
export function createIdleAiPickState(): AiPickState {
    return { phase: 'idle', pick: null, contextualDef: '', note: '', fallback: false };
}

/** 归一化缓存 key：句子 trim（大小写保留，语境有意义），单词 trim 并小写化 */
export function makeAiPickCacheKey(sentence: string, word: string): string {
    return `${sentence.trim()}|${word.trim().toLowerCase()}`;
}

/** 使指定“句子|单词”的缓存结果失效（候选集变化后需要重新优选时使用） */
export function invalidateAiPickCache(sentence: string, word: string): void {
    resultCache.delete(makeAiPickCacheKey(sentence, word));
}

/**
 * 发起一次 AI 优选。Promise 在最终状态（done/error/缓存命中/被中止）回调后 resolve；
 * 所有可预期的失败（含中止）都已在内部处理，不会 reject。
 */
export async function requestAiPick(options: RequestAiPickOptions): Promise<void> {
    const { sentence, word, config, signal, onUpdate } = options;
    const cacheKey = makeAiPickCacheKey(sentence, word);
    const cached = resultCache.get(cacheKey);
    if (cached != null) {
        onUpdate({ ...cached });
        return;
    }
    const state: AiPickState = createIdleAiPickState();
    state.phase = 'loading';
    onUpdate({ ...state });

    const emitError = (error: unknown): void => {
        if (error instanceof LlmError && error.kind === 'aborted') {
            return; // 外部中止：静默结束，不推进 error 态
        }
        console.error('AI 优选释义失败:', error);
        state.phase = 'error';
        onUpdate({ ...state });
    };

    // 1. 准备候选：优先使用调用方预取的结果（与界面条目索引对齐），否则内部并行查三本词典
    let sources: AiPickSources;
    if (options.sources != null) {
        sources = {
            collins: options.sources.collins ?? [],
            oxford: options.sources.oxford ?? [],
            youdao: options.sources.youdao ?? []
        };
    } else {
        sources = await fetchAllDictSources(word);
    }
    const { lines, counts } = buildCandidates(sources);
    const candidateTotal = counts.collins + counts.oxford + counts.youdao;
    console.debug(`[aiPick] 送入候选 collins=${counts.collins} oxford=${counts.oxford} youdao=${counts.youdao} total=${candidateTotal}`);

    // 2. 组装消息
    const messages: ChatMessage[] = [
        { role: 'system', content: SYSTEM_PROMPT },
        { role: 'user', content: buildUserPrompt(sentence, word, lines) }
    ];

    // 3. 流式请求（含 reasoningEffort 兼容重试）
    const onDelta = (fullText: string): void => {
        // 首个 delta 到达：loading → streaming
        state.phase = 'streaming';
        // 增量提取 contextual_def 的部分字符串值做打字机效果；提取失败不中断流
        const partial = extractPartialStringField(fullText, 'contextual_def');
        if (partial != null) {
            state.contextualDef = partial;
        }
        onUpdate({ ...state });
    };
    const requestOptions = { messages, signal, onDelta, jsonMode: true, temperature: 0.2 };
    let fullText: string;
    try {
        fullText = await streamChatCompletion(config, requestOptions);
    } catch (error) {
        // 部分服务不支持 reasoning_effort 字段（400/422）：去掉该字段后重试一次
        const canRetryWithoutReasoningEffort = error instanceof LlmError
            && error.kind === 'http'
            && (error.status === 400 || error.status === 422)
            && config.reasoningEffort != null
            && config.reasoningEffort.trim().length > 0;
        if (!canRetryWithoutReasoningEffort) {
            emitError(error);
            return;
        }
        try {
            fullText = await streamChatCompletion({ ...config, reasoningEffort: '' }, requestOptions);
        } catch (retryError) {
            emitError(retryError);
            return;
        }
    }

    // 4. 容错解析与校验
    console.debug(`[aiPick] 模型原始输出: ${truncateForLog(fullText)}`);
    let parsed: ParsedAiPick;
    try {
        parsed = parseAiPickResult(fullText, counts);
    } catch (error) {
        emitError(error);
        return;
    }
    // 候选非空但模型未指向有效候选（易被误判成 fallback）：追加提醒后重试一次，仍无效才接受现状
    if (parsed.pick == null && candidateTotal > 0) {
        console.warn('[aiPick] 候选非空但模型未选出有效候选，追加提醒后重试一次');
        const retryMessages: ChatMessage[] = [
            { role: 'system', content: SYSTEM_PROMPT },
            {
                role: 'user',
                content: buildUserPrompt(sentence, word, lines)
                    + '\n\n注意：候选列表非空，除非全部明显不相关，否则必须选择一个候选编号。'
            }
        ];
        try {
            const retryText = await streamChatCompletion(config, { ...requestOptions, messages: retryMessages });
            console.debug(`[aiPick] 重试的模型原始输出: ${truncateForLog(retryText)}`);
            parsed = parseAiPickResult(retryText, counts);
        } catch (retryError) {
            if (retryError instanceof LlmError && retryError.kind === 'aborted') {
                emitError(retryError); // 外部中止：静默结束
                return;
            }
            // 重试请求失败不拖垮整体：沿用首次解析结果（fallback）
            console.warn('[aiPick] 重试请求失败，沿用首次解析结果:', retryError);
        }
    }
    console.debug(`[aiPick] 最终判定: pick=${parsed.pick != null ? `${parsed.pick.source}#${parsed.pick.index}` : 'null'} fallback=${parsed.fallback ? 'true' : 'false'} note=${parsed.note.length > 0 ? '非空' : '空'}`);
    state.phase = 'done';
    state.pick = parsed.pick;
    state.contextualDef = parsed.contextualDef;
    state.note = parsed.note;
    state.fallback = parsed.fallback;
    onUpdate({ ...state });
    cacheResult(cacheKey, state);
}

/** 并行查询三本词典；单本失败按空结果处理，不拖垮整体（全部失败即无候选） */
async function fetchAllDictSources(word: string): Promise<AiPickSources> {
    const [collins, oxford, youdao] = await Promise.allSettled([
        dict.searchCollins(word),
        dict.searchOxford(word),
        dict.searchYoudao(word)
    ]);
    const unwrap = <T>(result: PromiseSettledResult<T[]>): T[] =>
        result.status === 'fulfilled' ? result.value : [];
    return {
        collins: unwrap(collins),
        oxford: unwrap(oxford),
        youdao: unwrap(youdao)
    };
}

/** 去掉 HTML 标签（如 Collins 英文释义中的 <b>）并压缩空白，使候选成为单行文本 */
function toSingleLine(text: string | null): string {
    return (text ?? '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
}

/** 用 ' | ' 拼接非空片段 */
function joinParts(parts: string[]): string {
    return parts.filter(part => part.length > 0).join(' | ');
}

/** 截断长文本用于诊断日志（默认上限 800 字符） */
function truncateForLog(text: string, max = 800): string {
    return text.length > max ? `${text.slice(0, max)}…（共 ${text.length} 字符）` : text;
}

/**
 * 组装候选：按词典分组编号（collins#0..n、oxford#0..n、youdao#0..n），
 * 编号即该词典候选列表中的索引（与界面条目索引一致）；每本词典最多取前
 * MAX_CANDIDATES[source] 条以控制 token（有道条目短，上限更宽）。
 *
 * @returns lines 送入 prompt 的候选行；counts 各词典实际送入的条数（校验模型返回的 index 用）
 */
function buildCandidates(sources: AiPickSources): { lines: string[]; counts: Record<DictSource, number>; } {
    const lines: string[] = [];
    const counts: Record<DictSource, number> = { collins: 0, oxford: 0, youdao: 0 };
    sources.collins.slice(0, MAX_CANDIDATES.collins).forEach((item, index) => {
        const text = joinParts([toSingleLine(item.sense), toSingleLine(item.enDef), toSingleLine(item.cnDef)]);
        lines.push(`collins#${index}: ${text.length > 0 ? text : '（无释义）'}`);
        counts.collins += 1;
    });
    sources.oxford.slice(0, MAX_CANDIDATES.oxford).forEach((item, index) => {
        const head = joinParts([toSingleLine(item.phrase), toSingleLine(item.sense), toSingleLine(item.ext)]);
        const text = joinParts([head, toSingleLine(item.enDef), toSingleLine(item.cnDef)]);
        lines.push(`oxford#${index}: ${text.length > 0 ? text : '（无释义）'}`);
        counts.oxford += 1;
    });
    sources.youdao.slice(0, MAX_CANDIDATES.youdao).forEach((item, index) => {
        const typeTag = item.meaningType === 'web' ? '[网络释义] '
            : item.meaningType === 'phrase' ? '[短语] ' : '';
        const text = joinParts([toSingleLine(item.sense), toSingleLine(item.cnDef)]);
        lines.push(`youdao#${index}: ${typeTag}${text.length > 0 ? text : '（无释义）'}`);
        counts.youdao += 1;
    });
    return { lines, counts };
}

function buildUserPrompt(sentence: string, word: string, candidateLines: string[]): string {
    const candidatesText = candidateLines.length > 0 ? candidateLines.join('\n') : '（无候选）';
    return `句子：${sentence}\n目标词：${word}\n\n候选义项：\n${candidatesText}`;
}

/**
 * 容错提取 JSON 对象：模型可能用 ```json 围栏包裹或夹带前后文字，
 * 先剥离围栏，再截取首尾花括号之间的内容做 JSON.parse；
 * 失败时抛出 kind 为 'bad_response' 的 LlmError。
 */
export function extractJsonObject(text: string): unknown {
    let trimmed = text.trim();
    const fence = /```(?:json)?\s*([\s\S]*?)```/.exec(trimmed);
    if (fence != null && fence[1] != null) {
        trimmed = fence[1].trim();
    }
    const start = trimmed.indexOf('{');
    const end = trimmed.lastIndexOf('}');
    if (start === -1 || end === -1 || end <= start) {
        throw new LlmError('bad_response', undefined, 'no JSON object found in AI output');
    }
    try {
        return JSON.parse(trimmed.slice(start, end + 1));
    } catch (error) {
        throw new LlmError('bad_response', undefined, `failed to parse AI output as JSON: ${String(error)}`);
    }
}

/**
 * 解析并校验 AI 输出。
 * - pick.source/index 必须指向真实存在的候选，否则置为 null；
 * - pick 为 null 但 contextualDef 非空时视为 fallback（AI 凭自身知识生成）；
 * - pick 为 null 且 contextualDef 为空时抛出 'bad_response'。
 */
export function parseAiPickResult(text: string, counts: Record<DictSource, number>): ParsedAiPick {
    const raw = extractJsonObject(text);
    if (raw == null || typeof raw !== 'object' || Array.isArray(raw)) {
        throw new LlmError('bad_response', undefined, 'AI output is not a JSON object');
    }
    const obj = raw as Record<string, unknown>;
    const rawDef = obj['contextual_def'];
    const rawNote = obj['note'];
    const contextualDef = typeof rawDef === 'string' ? rawDef.trim() : '';
    const note = typeof rawNote === 'string' ? rawNote.trim() : '';
    const pick = normalizePick(obj['pick'], counts);
    let fallback = obj['fallback'] === true;
    if (pick == null && contextualDef.length > 0) {
        // 未指向有效候选但给出了释义：按 AI 生成处理
        fallback = true;
    }
    if (pick == null && contextualDef.length === 0) {
        throw new LlmError('bad_response', undefined, 'AI output has neither a valid pick nor a contextual_def');
    }
    return { pick, contextualDef, note, fallback };
}

/** 校验模型返回的 pick 是否指向真实存在的候选；越界/非法时返回 null */
function normalizePick(raw: unknown, counts: Record<DictSource, number>): AiPick | null {
    if (raw == null || typeof raw !== 'object') {
        return null;
    }
    const rawSource = (raw as { source?: unknown; }).source;
    const rawIndex = (raw as { index?: unknown; }).index;
    // source 宽容化：忽略大小写与首尾空白（如 "Collins"）
    const source = typeof rawSource === 'string' ? rawSource.trim().toLowerCase() : '';
    if (source !== 'collins' && source !== 'oxford' && source !== 'youdao') {
        return null;
    }
    // index 宽容化：接受纯数字字符串（如 "2"）
    const index = typeof rawIndex === 'number' ? rawIndex
        : typeof rawIndex === 'string' && /^\d+$/.test(rawIndex.trim()) ? Number(rawIndex.trim())
            : NaN;
    if (!Number.isInteger(index) || index < 0 || index >= counts[source]) {
        return null;
    }
    return { source, index };
}

/**
 * 从流式累计文本中提取字符串字段的（可能尚未闭合的）值，用于打字机式增量显示。
 * 提取不到或部分值解析失败时返回 null（调用方仅推进 phase，不中断流）。
 */
function extractPartialStringField(text: string, key: string): string | null {
    const pattern = new RegExp(`"${key}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)`);
    const match = pattern.exec(text);
    if (match == null || match[1] == null) {
        return null;
    }
    return decodePartialJsonString(match[1]);
}

/** 解析可能截断的 JSON 字符串内容；尾部有不完整转义序列时先去掉再解析，仍失败则返回 null */
function decodePartialJsonString(raw: string): string | null {
    try {
        return JSON.parse(`"${raw}"`) as string;
    } catch {
        // 去掉末尾可能不完整的转义序列（如孤立的 '\' 或 '\u4e2'）后重试
        const trimmed = raw.replace(/\\(?:u[0-9a-fA-F]{0,3})?$/, '');
        if (trimmed.length === raw.length) {
            return null;
        }
        try {
            return JSON.parse(`"${trimmed}"`) as string;
        } catch {
            return null;
        }
    }
}

/** 写入缓存并维持容量上限：超出时按插入序逐出最旧条目 */
function cacheResult(key: string, state: AiPickState): void {
    resultCache.delete(key); // 先删后插，刷新插入顺序
    resultCache.set(key, { ...state });
    while (resultCache.size > CACHE_MAX_ENTRIES) {
        const oldest = resultCache.keys().next().value;
        if (oldest === undefined) {
            break;
        }
        resultCache.delete(oldest);
    }
}
