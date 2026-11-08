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

/**
 * AI 优选的状态机阶段：
 * - fetching 表示正在取词典候选（有道是网络请求，可能等上几秒）；
 * - thinking 表示思考模型正在输出思维链（非思考模型跳过该阶段）
 */
export type AiPickPhase = 'idle' | 'fetching' | 'loading' | 'thinking' | 'streaming' | 'done' | 'error';

/**
 * 查询文本的类型，由模型判断。决定后续怎么处理：
 * - word / phrase：走词典优选，没有候选时生成释义；
 * - sentence：不做优选，直接给整句译文（复用同一套卡片字段）；
 * - misspelling：像词但拼错，只提示正确拼写，不生成释义；
 * - gibberish：乱码、随机字符、按键误触，不生成任何内容
 */
export type AiPickKind = 'word' | 'phrase' | 'sentence' | 'misspelling' | 'gibberish';

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
    /** 查询文本的类型；misspelling / gibberish 时没有可加入 Anki 的内容，但结论仍要展示 */
    kind: AiPickKind;
    /** 失败原因（phase 为 error 时有值）：卡片上要说明为什么没有结果，而不是悄悄消失 */
    errorMessage: string;
    /**
     * 候选来源的问题：查询失败或被跳过。非空说明候选不完整，AI 生成的释义未经核对，
     * 需要在卡片和入库内容里如实标注（而不是一律写成“词典未收录”）。
     * 连续返空那类问题属于会话级统计，不在这里
     */
    sourceIssues: AiPickSourceIssue[];
}

/**
 * 单本词典的查询结果：
 * - failed 表示请求失败；
 * - skipped 表示连续失败后被熔断跳过；
 * - abandoned 表示查询太慢，用户手动跳过了等待（请求仍在后台跑）
 */
export type DictFetchStatus = 'ok' | 'failed' | 'skipped' | 'abandoned';

/** 三本词典各自的查询结果 */
export type AiPickSourcesStatus = Record<DictSource, DictFetchStatus>;

/**
 * 卡片上要提示的“候选来源问题”：本次查询失败，或该词典已连续多次在别的词典有结果时返空。
 * 后者只陈述事实（连续 N 次返空），不替用户推断“是不是故障”
 */
export interface AiPickSourceIssue {
    source: DictSource;
    kind: 'failed' | 'skipped' | 'abandoned' | 'empty';
    /** kind 为 'empty' 时的连续返空次数 */
    count: number;
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
    kind: AiPickKind;
    /** 模型原样给出的 pick（未校验）：null 表示明确没选，非 null 表示给了值但没对上候选 */
    rawPick: string | null;
}

/**
 * 是否值得为“没选中候选”再发一次请求。
 *
 * 只在模型给了编号却对不上时重试——那是一次输出错误（抄错、编造），提醒一遍有机会修好。
 * 明确返回 null 是模型的判断（它认为候选都不合适），重试等于逼它推翻自己，逼出来的多半是
 * 一个错选；错选又不会在界面上暴露，比“按 AI 生成处理”糟糕得多。
 *
 * 整句/拼写错误/乱码本来就没有可选的候选，不重试。
 */
export function shouldRetryWithNudge(parsed: ParsedAiPick, candidateTotal: number): boolean {
    return parsed.pick == null
        && parsed.rawPick != null
        && candidateTotal > 0
        && (parsed.kind === 'word' || parsed.kind === 'phrase');
}

/**
 * 本次结果是否有可加入 Anki 的内容：命中了词典条目，或者拿到了可写的释义/译文。
 * 拼写错误与乱码没有内容可写，卡片上也不该出现加号
 */
export function isAiPickAddable(state: AiPickState): boolean {
    if (state.pick != null) {
        return true;
    }
    if (state.kind === 'misspelling' || state.kind === 'gibberish') {
        return false;
    }
    return state.contextualDef.length > 0;
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
    /**
     * 各词典的查询结果，缺省视为全部成功。查询失败不会阻断生成（有道长期故障时正是
     * fallback 最有用的场景），只用来把“词典未收录”和“词典没查到”在标注上区分开
     */
    sourcesStatus?: Partial<AiPickSourcesStatus>;
    /** 状态推进回调；每次回调传入一份完整的状态快照 */
    onUpdate: (state: AiPickState) => void;
}

/**
 * 每本词典最多送入 LLM 的候选条数。
 *
 * 只作为 token 的兜底保护，不再当筛选手段：常见词条的义项数量远小于这个上限，
 * 而“取前 N 条”会把排在第 N 条之后的正确义项悄悄丢掉——那是最难被发现的一类错。
 * 有道条目多且杂，超限时优先保留词典级的简明释义与短语，网络释义排在最后
 */
const MAX_CANDIDATES: Record<DictSource, number> = { collins: 40, oxford: 40, youdao: 30 };

const SOURCES: readonly DictSource[] = ['collins', 'oxford', 'youdao'];
/** 内存缓存的最大条目数，超出时逐出最旧条目 */
const CACHE_MAX_ENTRIES = 200;

const SYSTEM_PROMPT = `你是词典义项判别助手。给定英文句子、查询文本，以及来自多部词典的编号候选义项，判断这段查询文本在此句中的意思。

只输出 JSON，不要输出其他任何文字：
{"kind":"word|phrase|sentence|misspelling|gibberish","pick":"<候选编号>"|null,"contextual_def":"...","note":"..."}

kind 按查询文本本身判断：
- word：真实存在的单词，含过去式、复数等屈折形式
- phrase：两个词以上的固定搭配、习语、术语
- sentence：完整的句子或从句片段
- misspelling：看起来是一个词但拼错了，你能指出正确拼写
- gibberish：乱码、随机字母组合、按键误触，无法解释

其余字段：
- pick 填候选行首方括号里的编号，逐字复制该编号（候选行为 "[co3f9] collins: …" 时应填 "co3f9"）；不要自行编号，也不要输出候选行以外的编号。kind 为 word/phrase 时指向最契合语境的候选，候选非空且存在基本契合的义项就必须选中；其余 kind 或没有合适候选时为 null。
- contextual_def：word/phrase 时给该词在此句中的简明中文释义，不超过 20 字；sentence 时给整句中文译文，不超过 60 字，自然通顺、不要逐词直译；misspelling/gibberish 时给空字符串 ""。
- note：一句搭配、用法或易混提示，不超过 30 字；misspelling 时给出正确拼写；gibberish 时说明无法识别；没有可说时给空字符串 ""。
- 候选为空不等于文本无效：生僻词、专有名词、俚语、短语照常判为 word/phrase。`;

/** 结果内存缓存（仅缓存 done 态），key 见 makeAiPickCacheKey */
const resultCache = new Map<string, AiPickState>();

/** 清空结果缓存（dev 场景切换/重置用；也让测试能从干净状态开始） */
export function clearAiPickCache(): void {
    resultCache.clear();
}

/** 生成一份初始（idle）状态 */
export function createIdleAiPickState(): AiPickState {
    return {
        phase: 'idle', pick: null, contextualDef: '', note: '',
        fallback: false, kind: 'word', sourceIssues: [], errorMessage: '',
    };
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
    let sourceIssues = sourceIssuesOf(options.sourcesStatus);
    // 缓存命中要求来源状态也一致：上次有道失败时记下的结果，不能在它恢复后继续复用
    const cached = resultCache.get(cacheKey);
    if (cached != null && sameSourceIssues(cached.sourceIssues, sourceIssues)) {
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
        state.errorMessage = truncateForLog(String(error instanceof Error ? error.message : error), 160);
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
        const fetched = await fetchAllDictSources(word);
        sources = fetched.sources;
        sourceIssues = fetched.sourceIssues;
    }
    const { lines, counts, byId } = buildCandidates(sources);
    const candidateTotal = counts.collins + counts.oxford + counts.youdao;
    console.debug(`[aiPick] 送入候选 collins=${counts.collins} oxford=${counts.oxford} youdao=${counts.youdao} total=${candidateTotal}`);

    // 来源不全不阻断生成：有道长期故障时，正是“词典查不到、需要 AI 兜底”的词最受影响。
    // 影响只体现在标注上——候选不完整时，AI 生成的释义要写明“未核对”
    if (sourceIssues.length > 0) {
        const detail = sourceIssues.map(issue => `${issue.source}(${issue.kind})`).join(', ');
        console.warn(`[aiPick] 候选来源不完整：${detail}；生成结果将标注为未核对`);
    }

    // 2. 组装消息
    const messages: ChatMessage[] = [
        { role: 'system', content: SYSTEM_PROMPT },
        { role: 'user', content: buildUserPrompt(sentence, word, lines) }
    ];

    // 3. 流式请求（400/422 的参数兼容降级重试与两段式超时均由 streamChatCompletion 内部处理）
    const onReasoning = (): void => {
        // 首个思维链增量到达：loading → thinking（仅推进一次，后续增量不重复触发 UI 更新）
        if (state.phase === 'loading') {
            state.phase = 'thinking';
            console.debug('[aiPick] 模型开始输出思维链');
            onUpdate({ ...state });
        }
    };
    const onDelta = (fullText: string): void => {
        // 首个正文 delta 到达：loading/thinking → streaming
        state.phase = 'streaming';
        // pick 字段在 JSON 中排最前：提前部分解析出被选条目，让 UI 尽早渲染；只设置一次
        if (state.pick == null) {
            const partialPick = extractPartialPick(fullText, byId);
            if (partialPick != null) {
                state.pick = partialPick;
                console.debug(`[aiPick] 流式提前解析出 pick: ${partialPick.source}#${partialPick.index}`);
            }
        }
        // 增量提取 note 的部分字符串值
        const partialNote = extractPartialStringField(fullText, 'note');
        if (partialNote != null) {
            state.note = partialNote;
        }
        // 增量提取 contextual_def 的部分字符串值做打字机效果（fallback 路径用）；提取失败不中断流
        const partial = extractPartialStringField(fullText, 'contextual_def');
        if (partial != null) {
            state.contextualDef = partial;
        }
        onUpdate({ ...state });
    };
    // 超时沿用默认的两段式（首字节 + 流空闲，另有宽松总上限）：思考模型的思维链可能很长，
    // 只要还在持续输出就不应被中断；用户可随时通过 signal 停止。
    // 不设 temperature：新一代模型普遍只接受服务端默认值，且实测推理模型的输出随机性
    // 来自思维链本身，调温度既不改多样性也不改稳定性
    const requestOptions = {
        messages, signal, onDelta, onReasoning,
        jsonMode: true,
        jsonSchema: buildResponseSchema(byId),
        temperature: config.temperature,
    };
    let fullText: string;
    try {
        fullText = await streamChatCompletion(config, requestOptions);
    } catch (error) {
        emitError(error);
        return;
    }

    // 4. 容错解析与校验
    console.debug(`[aiPick] 模型原始输出: ${truncateForLog(fullText)}`);
    let parsed: ParsedAiPick;
    try {
        parsed = parseAiPickResult(fullText, byId);
    } catch (error) {
        emitError(error);
        return;
    }
    if (shouldRetryWithNudge(parsed, candidateTotal)) {
        console.warn(`[aiPick] 送入 ${candidateTotal} 条候选，但模型给的 pick="${parsed.rawPick ?? ''}" 对不上，追加提醒后重试一次`);
        // 重试是第二次请求，必须让界面重新表现为“开始生成”：否则第一轮的流式内容留在卡片上，
        // 第二轮的增量从短变长，看起来像“第一遍没结束就被覆盖”
        state.phase = 'loading';
        state.pick = null;
        state.contextualDef = '';
        state.note = '';
        state.sourceIssues = [];
        state.errorMessage = '';
        onUpdate({ ...state });
        const retryMessages: ChatMessage[] = [
            { role: 'system', content: SYSTEM_PROMPT },
            {
                role: 'user',
                content: buildUserPrompt(sentence, word, lines)
                    + '\n\n注意：pick 必须逐字复制候选行首方括号内的编号，且只能是候选列表里出现过的编号。'
            }
        ];
        try {
            const retryText = await streamChatCompletion(config, { ...requestOptions, messages: retryMessages });
            console.debug(`[aiPick] 重试的模型原始输出: ${truncateForLog(retryText)}`);
            parsed = parseAiPickResult(retryText, byId);
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
    state.kind = parsed.kind;
    state.sourceIssues = sourceIssues;
    onUpdate({ ...state });
    cacheResult(cacheKey, state);
}

/** 并行查询三本词典；单本失败按空结果处理并记为 failed，不拖垮整体（全部失败即无候选） */
async function fetchAllDictSources(word: string): Promise<{ sources: AiPickSources; sourceIssues: AiPickSourceIssue[]; }> {
    const [collins, oxford, youdao] = await Promise.allSettled([
        dict.searchCollins(word),
        dict.searchOxford(word),
        dict.searchYoudao(word)
    ]);
    const unwrap = <T>(result: PromiseSettledResult<T[]>): T[] =>
        result.status === 'fulfilled' ? result.value : [];
    const sourceIssues: AiPickSourceIssue[] = [];
    const statusOf = (source: DictSource, result: PromiseSettledResult<unknown[]>): void => {
        if (result.status === 'rejected') {
            sourceIssues.push({ source, kind: 'failed', count: 0 });
        }
    };
    statusOf('collins', collins);
    statusOf('oxford', oxford);
    statusOf('youdao', youdao);
    return {
        sources: {
            collins: unwrap(collins),
            oxford: unwrap(oxford),
            youdao: unwrap(youdao)
        },
        sourceIssues
    };
}

/** 把调用方传入（或缺省）的各词典状态归一成来源问题列表 */
function sourceIssuesOf(status: Partial<AiPickSourcesStatus> | undefined): AiPickSourceIssue[] {
    const all: DictSource[] = ['collins', 'oxford', 'youdao'];
    return all
        .filter(source => status?.[source] !== undefined && status[source] !== 'ok')
        .map(source => {
            const status_ = status?.[source];
            const kind = status_ === 'skipped' || status_ === 'abandoned' ? status_ : 'failed';
            return { source, kind, count: 0 };
        });
}

/** 两次请求的来源问题集合是否一致（顺序无关） */
function sameSourceIssues(a: AiPickSourceIssue[], b: AiPickSourceIssue[]): boolean {
    return a.length === b.length
        && a.every(x => b.some(y => y.source === x.source && y.kind === x.kind));
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
 * 候选的短编号：由“来源 + 序号 + 释义文本”哈希得到的 6 位字符串（如 co3f9）。
 *
 * 不用 0/1/2 这样的数组下标：模型对小数有很强的先验，指向错误位置时既不会报错也看不出来
 * （它可能一边推理出“第 2 条”一边输出 index=3）。编号不可猜，模型只能逐字复制候选行首的
 * 那串字符，抄错就等于指向不存在的条目，能被校验直接发现
 */
function candidateId(source: DictSource, index: number, text: string): string {
    const seed = `${source}\u0000${index}\u0000${text}`;
    let hash = 0x811c9dc5;
    for (let i = 0; i < seed.length; i++) {
        hash ^= seed.charCodeAt(i);
        hash = Math.imul(hash, 0x01000193) >>> 0;
    }
    return `${source.slice(0, 2)}${hash.toString(36).padStart(5, '0')}`;
}

/** 一条待送出的候选：index 是它在界面列表中的位置，priority 越小越优先保留 */
interface CandidateRow {
    index: number;
    text: string;
    priority: number;
}

/** 归一化释义文本，用于识别同一义项（只合并去掉空白与标点后完全相同的） */
function dedupeKey(text: string): string {
    return text.toLowerCase().replace(/[\s\p{P}]+/gu, '');
}

/**
 * 组装候选：跨词典去重 → 有道按类型排序 → 三本词典交错排列。
 *
 * - 去重：同一义项在多本词典里重复出现时，模型被迫二选一，同一语境两次调用的结果
 *   可能落在不同的词典上。只合并文本完全相同的，措辞不同的一律保留；
 * - 交错：原来按词典分块排列，排在最前的柯林斯被系统性偏爱；交错后位置不再是信号；
 * - 编号：行首是不透明短编号，模型只能逐字复制，界面按 byId 映射回列表索引。
 *
 * @returns lines 送入 prompt 的候选行；counts 各词典实际送入的条数；byId 编号 → 候选位置
 */
export function buildCandidates(sources: AiPickSources): {
    lines: string[];
    counts: Record<DictSource, number>;
    byId: Map<string, AiPick>;
} {
    const rows: Record<DictSource, CandidateRow[]> = {
        collins: sources.collins.map((item, index) => ({
            index,
            priority: 0,
            text: joinParts([toSingleLine(item.sense), toSingleLine(item.enDef), toSingleLine(item.cnDef)]),
        })),
        oxford: sources.oxford.map((item, index) => ({
            index,
            priority: 0,
            text: joinParts([
                joinParts([toSingleLine(item.phrase), toSingleLine(item.sense), toSingleLine(item.ext)]),
                toSingleLine(item.enDef),
                toSingleLine(item.cnDef),
            ]),
        })),
        youdao: sources.youdao.map((item, index) => ({
            index,
            priority: item.meaningType === 'concise' ? 0 : item.meaningType === 'phrase' ? 1 : 2,
            text: `${item.meaningType === 'web' ? '[网络释义] '
                : item.meaningType === 'phrase' ? '[短语] ' : ''}`
                + joinParts([toSingleLine(item.sense), toSingleLine(item.cnDef)]),
        })),
    };

    // 稳定排序：同类型内部保持词典原始顺序
    rows.youdao.sort((a, b) => a.priority - b.priority);

    // 按 collins → oxford → youdao 的先后去重并截断：先出现的留下
    const seen = new Set<string>();
    const kept: Record<DictSource, CandidateRow[]> = { collins: [], oxford: [], youdao: [] };
    for (const source of SOURCES) {
        for (const row of rows[source]) {
            const key = dedupeKey(row.text);
            if (key.length > 0) {
                if (seen.has(key)) {
                    continue;
                }
                seen.add(key);
            }
            if (kept[source].length >= MAX_CANDIDATES[source]) {
                continue;
            }
            kept[source].push(row);
        }
    }

    const lines: string[] = [];
    const counts: Record<DictSource, number> = { collins: 0, oxford: 0, youdao: 0 };
    const byId = new Map<string, AiPick>();
    const rounds = Math.max(kept.collins.length, kept.oxford.length, kept.youdao.length);
    for (let round = 0; round < rounds; round++) {
        for (const source of SOURCES) {
            const row = kept[source][round];
            if (row == null) {
                continue;
            }
            const text = row.text.length > 0 ? row.text : '（无释义）';
            const id = candidateId(source, row.index, text);
            byId.set(id, { source, index: row.index });
            lines.push(`[${id}] ${source}: ${text}`);
            counts[source] += 1;
        }
    }
    return { lines, counts, byId };
}

/**
 * 结构化输出的 schema：pick 只能取本次送入的候选编号或 null。
 * 支持 strict 模式的端点会据此约束解码，"编号非法"从此不可能出现；
 * 不支持的服务会降级成 json_object，仍然靠白名单校验兜住
 */
export function buildResponseSchema(byId: Map<string, AiPick>): { name: string; schema: Record<string, unknown>; } {
    const ids = [...byId.keys()];
    return {
        name: 'ai_pick',
        schema: {
            type: 'object',
            additionalProperties: false,
            required: ['kind', 'pick', 'contextual_def', 'note'],
            properties: {
                kind: {
                    type: 'string',
                    enum: ['word', 'phrase', 'sentence', 'misspelling', 'gibberish'],
                },
                pick: ids.length > 0
                    ? { anyOf: [{ type: 'string', enum: ids }, { type: 'null' }] }
                    : { type: 'null' },
                contextual_def: { type: 'string' },
                note: { type: 'string' },
            },
        },
    };
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
 * - pick 必须是本次候选里真实存在的编号，否则置为 null；
 * - pick 有效时一律按命中处理（有真实出处），并丢弃模型顺带写的释义；
 * - is_word 为 false 时按“未收录”处理：不给释义、不可加卡，只保留结论与说明；
 * - 其余情况下 pick 为 null 即 fallback，contextualDef 为空则视为 'bad_response'。
 */
export function parseAiPickResult(text: string, byId: Map<string, AiPick>): ParsedAiPick {
    const raw = extractJsonObject(text);
    if (raw == null || typeof raw !== 'object' || Array.isArray(raw)) {
        throw new LlmError('bad_response', undefined, 'AI output is not a JSON object');
    }
    const obj = raw as Record<string, unknown>;
    const rawDef = obj['contextual_def'];
    const rawNote = obj['note'];
    const contextualDef = typeof rawDef === 'string' ? rawDef.trim() : '';
    const note = typeof rawNote === 'string' ? rawNote.trim() : '';
    const kind = normalizeKind(obj['kind']);
    const rawPickValue = obj['pick'];
    const rawPick = typeof rawPickValue === 'string'
        ? rawPickValue.trim()
        : rawPickValue == null ? null : JSON.stringify(rawPickValue).slice(0, 40);
    const pick = lookupPick(rawPickValue, byId);
    if (pick != null) {
        // 指向了有效候选：有真实出处，无论模型如何标记都按命中处理，并丢弃其自行生成的释义
        return { pick, contextualDef: '', note, fallback: false, kind, rawPick };
    }
    if (kind === 'misspelling' || kind === 'gibberish') {
        return { pick: null, contextualDef: '', note, fallback: false, kind, rawPick };
    }
    if (contextualDef.length === 0) {
        throw new LlmError('bad_response', undefined, 'AI output has neither a valid pick nor a contextual_def');
    }
    return { pick: null, contextualDef, note, fallback: true, kind, rawPick };
}

/** 归一化 kind：缺省或未知值按 word 处理（模型可能不输出该字段，不能因此判成无效输入） */
function normalizeKind(raw: unknown): AiPickKind {
    const value = typeof raw === 'string' ? raw.trim().toLowerCase() : '';
    return value === 'phrase' || value === 'sentence' || value === 'misspelling' || value === 'gibberish'
        ? value
        : 'word';
}

/**
 * 把模型返回的编号解析成候选位置：只认本次真正送进去的编号，
 * 抄错、编造、大小写不一致（统一按小写比对）都返回 null，由调用方按“未选中”处理
 */
function lookupPick(raw: unknown, byId: Map<string, AiPick>): AiPick | null {
    if (typeof raw !== 'string') {
        return null;
    }
    return byId.get(raw.trim().toLowerCase()) ?? null;
}

/**
 * 从流式累计文本中提前抠出 pick 编号（pick 在 JSON 中排最前，通常最早完整），
 * 让界面在正文抵达前就能渲染选中条目。仅 best-effort：最终以完整 parse 为准。
 */
function extractPartialPick(text: string, byId: Map<string, AiPick>): AiPick | null {
    const match = /"pick"\s*:\s*"([A-Za-z0-9]+)"/.exec(text);
    if (match == null || match[1] == null) {
        return null;
    }
    return lookupPick(match[1], byId);
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
