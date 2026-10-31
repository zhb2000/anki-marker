import { computed, reactive, watch, type Ref } from 'vue';

import * as utils from '../../logics/utils';
import * as aiPickLogic from '../../logics/aiPick';
import type * as cfg from '../../logics/config';
import { isLlmReady, parseMaxTokens, type LlmRequestConfig } from '../../logics/llm';
import type { CardStatus } from '../../components/CardStatus';
import type { StatusNotice } from '../../fluent-controls/status';
import type { WordSearch } from './useWordSearch';
import type { DictName, WordItems } from './types';

/** 手打触发 AI 优选的停顿时长：业内 300–500ms 是“感知即时”的甜点，LLM 更贵故取上限 */
const AI_PICK_TYPING_DEBOUNCE_MS = 500;
/** 手打触发 AI 优选的最短查询长度：单个字符没有优选意义（显式动作不受此限） */
const AI_PICK_MIN_TYPED_LENGTH = 2;

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
    /** 是否显示 AI 优选卡片（idle/error 时不渲染；thinking 为思考模型输出思维链的阶段） */
    const showAiPickCard = computed(() =>
        aiPick.phase === 'loading' || aiPick.phase === 'thinking' || aiPick.phase === 'streaming' || aiPick.phase === 'done');

    /** AI 优选卡片加号按钮的状态：跟随 AI 选中的那条词典条目的添加状态 */
    const aiPickCardStatus = computed<CardStatus>(() => {
        if (aiPick.pick == null) {
            return 'not-added';
        }
        return wordItems[aiPick.pick.source][aiPick.pick.index]?.status ?? 'not-added';
    });

    function resetAiPick() {
        Object.assign(aiPick, aiPickLogic.createIdleAiPickState());
        aiSearchedKey = '';
    }

    /** 判断指定词典的指定条目是否为当前 AI 优选命中的条目 */
    function isAiPicked(source: DictName, index: number): boolean {
        return aiPick.pick != null && aiPick.pick.source === source && aiPick.pick.index === index;
    }

    /**
     * AI 优选卡片上的就地反馈：就是 AI 选中的那条词典条目自己的提示（加卡进度、失败与重试、编辑降级）。
     * 从 AI 卡加卡时，源条目可能在隐藏的词典标签页或列表下方，只挂在源卡片上用户看不见
     */
    const aiPickNotice = computed<StatusNotice | null>(() => {
        if (aiPick.pick == null) {
            return null;
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
            reasoningEffort: config.llmReasoningEffort
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
        try {
            // 并行搜索三本词典；searchingOrSearchedWords 去重保证已查过的词典立即返回。
            // 预取失败（多为有道的 HTTP 抖动）时静默重试一次，避免候选缺失导致 AI 误走 fallback
            const prefetch = async (dictionary: DictName): Promise<void> => {
                let ok = await searchAndUpdate(trimmed, dictionary, { errorFeedback: 'silent' });
                if (!ok) {
                    console.warn(`[aiPick] ${dictionary} 预取失败，重试一次`);
                    ok = await searchAndUpdate(trimmed, dictionary, { errorFeedback: 'silent' });
                    if (!ok) {
                        console.warn(`[aiPick] ${dictionary} 候选缺失，本次优选将不含该来源`);
                    }
                }
            };
            await Promise.all([prefetch('collins'), prefetch('oxford'), prefetch('youdao')]);
            console.debug(`[aiPick] 候选数量 collins=${wordItems.collins.length} oxford=${wordItems.oxford.length} youdao=${wordItems.youdao.length}`);
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
        void maybeStartAiPick(searchText.value);
    }

    return {
        aiPick,
        showAiPickCard,
        aiPickCardStatus,
        aiPickNotice,
        aiPickItem,
        isAiPicked,
        debouncedAiPick,
        maybeStartAiPick,
        retriggerIfCandidatesGrew,
        stopAiPick,
        regenerateAiPick,
    };
}

export type AiPick = ReturnType<typeof useAiPick>;
