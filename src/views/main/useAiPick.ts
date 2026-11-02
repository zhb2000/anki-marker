import { computed, reactive, ref, watch, type Ref } from 'vue';

import * as utils from '../../logics/utils';
import * as aiPickLogic from '../../logics/aiPick';
import * as dictHealthLogic from '../../logics/dictHealth';
import { createPrefetchBreaker } from '../../logics/prefetchBreaker';
import type * as cfg from '../../logics/config';
import { isLlmReady, parseMaxTokens, parseTemperature, type LlmRequestConfig } from '../../logics/llm';
import type { CardStatus } from '../../components/CardStatus';
import type { StatusNotice } from '../../fluent-controls/status';
import type { WordSearch } from './useWordSearch';
import type { DictName, WordItems } from './types';

/** 手打触发 AI 优选的停顿时长：业内 300–500ms 是“感知即时”的甜点，LLM 更贵故取上限 */
const AI_PICK_TYPING_DEBOUNCE_MS = 500;
/** 手打触发 AI 优选的最短查询长度：单个字符没有优选意义（显式动作不受此限） */
const AI_PICK_MIN_TYPED_LENGTH = 2;
/** 自动预取的熔断参数：有道一次超时就是 10 秒，连续两次失败后跳过一分钟 */
const PREFETCH_TRIP_AFTER = 2;
const PREFETCH_COOLDOWN_MS = 60_000;
/**
 * 预取超过这个时长才提示“正在查哪本词典”。正常一次查询只要几百毫秒，
 * 提示来不及看清就消失，反而显得界面在闪
 */
const FETCH_SLOW_HINT_DELAY_MS = 700;

const DICTIONARIES: readonly DictName[] = ['collins', 'oxford', 'youdao'];

export interface UseAiPickOptions {
    sentence: Ref<string>;
    searchText: Ref<string>;
    wordItems: WordItems;
    searchAndUpdate: WordSearch['searchAndUpdate'];
    /** 页面初始化后才就绪的配置 */
    getConfig: () => cfg.Config;
}

/** AI 优选释义：卡片状态、请求的发起与中止 */
export function useAiPick(options: UseAiPickOptions) {
    const { sentence, searchText, wordItems, searchAndUpdate, getConfig } = options;

    /** AI 优选卡片的状态 */
    const aiPick = reactive<aiPickLogic.AiPickState>(aiPickLogic.createIdleAiPickState());
    /** 进行中的 AI 请求的中止器 */
    let aiAbort: AbortController | null = null;
    /** 当前 aiPick 状态对应的搜索 key（sentence|word），用于防止重复发起 */
    let aiSearchedKey = '';
    /** 本次 AI 请求送入的候选总数快照，用于检测切词典后候选变多（预取缺货）并补触发优选 */
    let aiCandidateTotal = 0;
    /**
     * AI 生成卡片（fallback）自己的条目状态。它没有对应的词典条目，无法挂在 wordItems 上，
     * 但同样需要加卡、移除、编辑与就地反馈
     */
    const aiGeneratedItem = reactive<{
        item: null;
        status: CardStatus;
        id: number | null;
        notice: StatusNotice | null;
    }>({ item: null, status: 'not-added', id: null, notice: null });
    /** aiGeneratedItem 对应的“句子|单词”；换 key 时才重置，避免同一次查询里重复搜索丢掉已添加状态 */
    let aiGeneratedKey = '';
    /** 各词典连续返空的会话级统计（接口静默失效时，单次空结果看不出来） */
    const dictHealth = dictHealthLogic.createDictHealth();
    /** 自动预取的熔断：只作用于 AI 之前的预取，用户明确发起的查询永远真的发出去 */
    const prefetchBreaker = createPrefetchBreaker({
        tripAfter: PREFETCH_TRIP_AFTER,
        cooldownMs: PREFETCH_COOLDOWN_MS,
    });
    /** 仍在等待的词典：超过提示延迟后才有值，用于“正在查询有道词典… 跳过” */
    const slowFetchSources = ref<DictName[]>([]);
    /** 本次预取的手动跳过入口（同一时刻只有一个 AI 请求） */
    let skipFetchWaiting: (() => void) | null = null;
    /** 是否显示 AI 优选卡片（idle/error 时不渲染；thinking 为思考模型输出思维链的阶段） */
    // error 也要留在界面上：失败原因必须让用户看见，否则卡片只是闪一下骨架屏就消失
    const showAiPickCard = computed(() =>
        aiPick.phase === 'fetching' || aiPick.phase === 'loading'
        || aiPick.phase === 'thinking' || aiPick.phase === 'streaming'
        || aiPick.phase === 'done' || aiPick.phase === 'error');

    /** AI 优选卡片加号按钮的状态：跟随 AI 选中的那条词典条目的添加状态 */
    const aiPickCardStatus = computed<CardStatus>(() => {
        if (aiPick.pick == null) {
            // 未命中词典条目时，加的是 AI 自己生成的那张卡
            return aiGeneratedItem.status;
        }
        return wordItems[aiPick.pick.source][aiPick.pick.index]?.status ?? 'not-added';
    });

    function resetAiPick() {
        Object.assign(aiPick, aiPickLogic.createIdleAiPickState());
        aiSearchedKey = '';
        // 旧请求可能还在后台跑，界面上的“正在查询… 跳过”必须先收起来
        slowFetchSources.value = [];
        skipFetchWaiting = null;
    }

    /**
     * 换了一次逻辑查询（句子或单词变化）时重置 AI 生成卡片的状态：它只属于上一条结果。
     * force 用于“同一个 key 重新生成”的场景——新释义对应的是另一张待加的卡，不能沿用旧的已添加状态
     */
    function resetAiGeneratedItem(key: string, force = false): void {
        if (!force && aiGeneratedKey === key) {
            return;
        }
        aiGeneratedKey = key;
        aiGeneratedItem.status = 'not-added';
        aiGeneratedItem.id = null;
        aiGeneratedItem.notice = null;
    }

    /** 判断指定词典的指定条目是否为当前 AI 优选命中的条目 */
    function isAiPicked(source: DictName, index: number): boolean {
        return aiPick.pick != null && aiPick.pick.source === source && aiPick.pick.index === index;
    }

    /**
     * AI 优选卡片上的就地反馈：就是 AI 选中的那条词典条目自己的提示（加卡进度、失败与重试、编辑降级）。
     * 从 AI 卡加卡时，源条目可能在隐藏的词典标签页或列表下方，只挂在源卡片上用户看不见
     */
    /**
     * 卡片上要提示的候选来源问题：本次失败的词典，以及连续多次返空的词典。
     * 返空只报事实与次数，不替用户下“词典坏了”的结论——用户可能只是在查生僻词
     */
    const aiPickSourceIssues = computed<aiPickLogic.AiPickSourceIssue[]>(() => {
        const issues: aiPickLogic.AiPickSourceIssue[] = [...aiPick.sourceIssues];
        DICTIONARIES.forEach(source => {
            if (issues.some(issue => issue.source === source)) {
                return; // 本次查询失败已经提示过了，不再叠加返空次数
            }
            if (dictHealthLogic.isEmptyStreakWorthNoticing(dictHealth, source)) {
                issues.push({ source, kind: 'empty', count: dictHealth.consecutiveEmpty[source] });
            }
        });
        return issues;
    });

    const aiPickNotice = computed<StatusNotice | null>(() => {
        if (aiPick.pick == null) {
            return aiGeneratedItem.notice;
        }
        return wordItems[aiPick.pick.source][aiPick.pick.index]?.notice ?? null;
    });

    /** AI 优选命中的词典条目（供 AiPickCard 渲染完整内容）；pick 为空或条目缺失时为 null */
    const aiPickItem = computed(() => {
        if (aiPick.pick == null) return null;
        const item = wordItems[aiPick.pick.source][aiPick.pick.index]?.item;
        return item != null ? { source: aiPick.pick.source, item } : null;
    });

    /**
     * 手打时的 AI 优选防抖：停手 `AI_PICK_TYPING_DEBOUNCE_MS` 才发起。
     * AI 是昂贵且慢的调用，而输入过程中的半截词结果必然作废——“只有最后一次输入有意义”
     * 正是 debounce 的语义（throttle 会让中间每一次都发）。显式动作不走这条路径，见 setQueryExplicitly。
     */
    const debouncedAiPick = utils.debounce((word: string) => {
        if (word.trim().length < AI_PICK_MIN_TYPED_LENGTH) {
            return;
        }
        void maybeStartAiPick(word);
    }, AI_PICK_TYPING_DEBOUNCE_MS);

    /**
     * 提交一次有效搜索后调用：若启用并配置好了 LLM，则并行搜索三本词典（填满 wordItems，
     * 保证候选与界面条目索引对齐、切 tab 即时）并发起 AI 优选；否则中止遗留请求并重置为 idle。
     */
    async function maybeStartAiPick(word: string) {
        // 显式发起（或本次防抖已到期）时取消待发的防抖，避免停手计时到期再发一次
        debouncedAiPick.cancel();
        const config = getConfig();
        const trimmed = word.trim();
        const llmConfig: LlmRequestConfig = {
            baseUrl: config.llmBaseUrl,
            apiKey: config.llmApiKey,
            model: config.llmModel,
            maxTokens: parseMaxTokens(config.llmMaxTokens),
            reasoningEffort: config.llmReasoningEffort,
            temperature: parseTemperature(config.llmTemperature)
        };
        if (trimmed.length === 0 || !config.llmEnabled || !isLlmReady(llmConfig)) {
            // 与 watch([sentence, searchText]) 的取消逻辑一致，作其他调用路径的兜底：
            // 文本已无效（空词/未启用/未配置），进行中的请求不会有人再取消，必须在此中止
            aiAbort?.abort();
            aiAbort = null;
            resetAiPick();
            return;
        }
        const key = aiPickLogic.makeAiPickCacheKey(sentence.value, trimmed);
        if (key === aiSearchedKey && aiPick.phase !== 'idle') {
            return; // 同一“句子+单词”已在进行或已有结果（含 error，不自动重试）
        }
        // 快速连点：中止旧请求
        aiAbort?.abort();
        const controller = new AbortController();
        aiAbort = controller;
        aiSearchedKey = key;
        resetAiGeneratedItem(key);
        // 进入“取候选”阶段：有道是网络请求，最坏要等 10 秒。先让卡片出来，
        // 等超过提示延迟还没回来时再说明在等哪本词典、并允许跳过
        Object.assign(aiPick, aiPickLogic.createIdleAiPickState(), { phase: 'fetching' });
        try {
            // 各词典的查询结果：失败/跳过不会阻断 AI，只用来把“未收录”和“没查到”在标注上分开
            const sourcesStatus: Record<DictName, aiPickLogic.DictFetchStatus> = {
                collins: 'ok', oxford: 'ok', youdao: 'ok',
            };
            const finished: Record<DictName, boolean> = { collins: false, oxford: false, youdao: false };
            let skippedByUser = false;
            const skipPromise = new Promise<void>(resolve => {
                skipFetchWaiting = () => {
                    skippedByUser = true;
                    resolve();
                };
            });
            const prefetch = async (dictionary: DictName): Promise<void> => {
                // 熔断只作用于自动预取：连续失败过的来源直接跳过，不再白等一个超时
                if (prefetchBreaker.shouldSkip(dictionary)) {
                    sourcesStatus[dictionary] = 'skipped';
                    finished[dictionary] = true;
                    console.warn(`[aiPick] ${dictionary} 连续失败，本次跳过查询`);
                    return;
                }
                const ok = await searchAndUpdate(trimmed, dictionary, { errorFeedback: 'silent' });
                finished[dictionary] = true;
                if (ok) {
                    prefetchBreaker.recordSuccess(dictionary);
                    return;
                }
                sourcesStatus[dictionary] = 'failed';
                prefetchBreaker.recordFailure(dictionary);
                console.warn(`[aiPick] ${dictionary} 查询失败，本次优选将不含该来源`);
            };
            slowFetchSources.value = [];
            const slowTimer = window.setTimeout(() => {
                // 期间可能已被新的查询接管：旧请求不该再往界面上写状态
                if (aiAbort !== controller) {
                    return;
                }
                const pending = DICTIONARIES.filter(dictionary => !finished[dictionary]);
                if (pending.length > 0) {
                    slowFetchSources.value = [...pending];
                }
            }, FETCH_SLOW_HINT_DELAY_MS);
            try {
                await Promise.race([
                    Promise.all(DICTIONARIES.map(async dictionary => prefetch(dictionary))),
                    skipPromise,
                ]);
            } finally {
                window.clearTimeout(slowTimer);
                // 只有当前这次请求仍有效时才清理共享状态，否则会抹掉新请求的提示与跳过入口
                if (aiAbort === controller) {
                    slowFetchSources.value = [];
                    skipFetchWaiting = null;
                }
            }
            if (skippedByUser) {
                // 用户等不下去了：没回来的词典按“跳过”处理，用已就绪的候选继续
                DICTIONARIES.forEach(dictionary => {
                    if (!finished[dictionary]) {
                        sourcesStatus[dictionary] = 'abandoned';
                    }
                });
                console.warn('[aiPick] 用户跳过等待，使用已就绪的候选');
            }
            console.debug(`[aiPick] 候选数量 collins=${wordItems.collins.length} oxford=${wordItems.oxford.length} youdao=${wordItems.youdao.length}`);
            // 记录各词典这次是“有结果 / 返空 / 失败 / 跳过”，用于发现“每次都为空但不报错”的接口静默失效
            const outcomes = (source: DictName): dictHealthLogic.DictQueryOutcome => {
                const status = sourcesStatus[source];
                if (status === 'failed') {
                    return 'failed';
                }
                if (status === 'skipped' || status === 'abandoned') {
                    // 两种“没等到结果”对健康度统计都是无效信号：既不算返空，也不清零
                    return 'skipped';
                }
                return wordItems[source].length > 0 ? 'filled' : 'empty';
            };
            dictHealthLogic.recordQuery(dictHealth, trimmed.toLowerCase(), {
                collins: outcomes('collins'),
                oxford: outcomes('oxford'),
                youdao: outcomes('youdao'),
            });
            // 等待期间若已有更新的搜索接管，则放弃本次发起
            if (aiAbort !== controller || controller.signal.aborted) {
                return;
            }
            aiCandidateTotal = wordItems.collins.length + wordItems.oxford.length + wordItems.youdao.length;
            await aiPickLogic.requestAiPick({
                sentence: sentence.value,
                word: trimmed,
                config: llmConfig,
                signal: controller.signal,
                sourcesStatus,
                sources: {
                    collins: wordItems.collins.map(model => model.item),
                    oxford: wordItems.oxford.map(model => model.item),
                    youdao: wordItems.youdao.map(model => model.item)
                },
                onUpdate: state => {
                    // 只接受当前请求的回调，丢弃被中止请求的陈旧回调
                    if (aiAbort === controller) {
                        Object.assign(aiPick, state);
                    }
                }
            });
        } finally {
            if (aiAbort === controller) {
                aiAbort = null;
            }
        }
    }

    /**
     * 切词典后候选总数较 AI 请求时的快照变多，说明此前预取缺货导致 AI 误走 fallback，
     * 值得用更全的候选自动补触发一次优选；候选数不再增长时不会重复触发，无循环风险
     */
    function retriggerIfCandidatesGrew(): void {
        const candidateTotal = wordItems.collins.length + wordItems.oxford.length + wordItems.youdao.length;
        if (aiPick.phase === 'done' && aiPick.fallback
            && searchText.value.trim().length > 0 && candidateTotal > aiCandidateTotal) {
            aiPickLogic.invalidateAiPickCache(sentence.value, searchText.value);
            resetAiPick();
            resetAiGeneratedItem(aiPickLogic.makeAiPickCacheKey(sentence.value, searchText.value), true);
            void maybeStartAiPick(searchText.value);
        }
    }

    /**
     * sentence 或 searchText 变化时，中止进行中的 AI 请求并重置卡片：
     * 进行中的结果只对旧的“句子+单词”有效，文本已变就该立即取消，而不是等新搜索发起时才中止
     * （否则 searchText 变空时 maybeStartAiPick 会静默早退，旧请求会继续跑完并把结果上屏）。
     *
     * 时序安全性：本 watch 在 flush 阶段执行，早于因本次文本变化而发起的新请求创建 controller
     * （maybeStartAiPick 要等 searchAndUpdate 的 await 之后才开始运行），因此不会误杀新请求；
     * maybeStartAiPick 入口的“快速连点”abort 仍保留，覆盖同文本重入（预取窗口内重复查询）的场景。
     */
    watch([sentence, searchText], () => {
        aiAbort?.abort();
        aiAbort = null;
        if (aiPick.phase !== 'idle') {
            resetAiPick();
        }
    });

    /**
     * 手动跳过等待：只放弃“等结果”，不取消已经发出的请求——它照常完成并填进词典列表。
     * 计作 skipped 而不是 failed：用户没耐心不等于词典故障，不能推动熔断
     */
    function skipAiFetch(): void {
        skipFetchWaiting?.();
    }

    /**
     * 停止按钮：中止进行中的 AI 请求并隐藏卡片。
     * 中止属静默结束（requestAiPick 内部对 aborted 不回调 error 态，onUpdate 有 controller 校验），
     * 此处立即重置即可；清空 aiSearchedKey 使同一“句子+单词”再次搜索时可重新触发。
     */
    function stopAiPick() {
        aiAbort?.abort();
        aiAbort = null;
        resetAiPick();
    }

    /**
     * 重新生成按钮：作废结果缓存后重置卡片并重新发起优选。
     * 仅 done 态可点（进行中显示的是停止按钮）；aiAbort 非空的防御分支理论上不可达。
     */
    function regenerateAiPick() {
        if (aiAbort != null) {
            return;
        }
        aiPickLogic.invalidateAiPickCache(sentence.value, searchText.value);
        resetAiPick();
        resetAiGeneratedItem(aiPickLogic.makeAiPickCacheKey(sentence.value, searchText.value), true);
        void maybeStartAiPick(searchText.value);
    }

    return {
        aiPick,
        aiGeneratedItem,
        aiPickSourceIssues,
        slowFetchSources,
        showAiPickCard,
        aiPickCardStatus,
        aiPickNotice,
        aiPickItem,
        isAiPicked,
        debouncedAiPick,
        maybeStartAiPick,
        retriggerIfCandidatesGrew,
        stopAiPick,
        skipAiFetch,
        regenerateAiPick,
    };
}

export type AiPick = ReturnType<typeof useAiPick>;
