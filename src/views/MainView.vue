<script setup lang="ts">
import { ref, reactive, watch, nextTick, onBeforeMount, computed } from 'vue';
import { useRouter } from 'vue-router';
import * as api from '../tauri-api';

import * as utils from '../logics/utils';
import * as dict from '../logics/dict';
import * as anki from '../logics/anki';
import * as cfg from '../logics/config';
import * as globals from '../logics/globals';
import * as preference from '../logics/preference';
import * as aiPickLogic from '../logics/aiPick';
import { isLlmReady, parseMaxTokens, type LlmRequestConfig } from '../logics/llm';
import { useSettingsStore } from '../logics/settings-store';
import AiPickCard from '../components/AiPickCard.vue';
import {
    FluentButton, FluentSelect, FluentInput, FluentTextarea, FluentRadio, FluentStatusText, dialog,
} from '../fluent-controls';
import type { StatusNotice } from '../fluent-controls/status';
import { useTransientNotice } from '../fluent-controls/useTransientNotice';
import {
    CardStatus,
    SentencePanel,
    SentenceCaptureRing,
    CollinsCard,
    OxfordCard,
    YoudaoCard,
    PlayAudioButton,
    SettingButton,
    ScrollMemory
} from '../components';


const router = useRouter();
const pageInitialized = ref(false);
let config: cfg.Config;
let ankiService: anki.AnkiService;

// #region 单词搜索
/** 单词列表的单词卡片 */
interface ItemModel<T> {
    item: T;
    status: CardStatus;
    id: number | null;
    /** 该卡片自己的就地反馈（连接进度、编辑降级提示等） */
    notice?: StatusNotice | null;
}

/** 卡片就地反馈的自动收起计时器（按条目对象持有，条目被替换时自然失效） */
const cardNoticeTimers = new WeakMap<object, number>();
/** 卡片就地反馈上的操作回调（“停止”“重试”），随提示一起替换 */
const cardNoticeActions = new WeakMap<object, () => void>();
/** 操作超过这个时长才显示进度与“停止”：正常速度的添加/移除不应闪一下 */
const SLOW_NOTICE_DELAY_MS = 700;
/** 写入失败后核对笔记时，按创建时间筛选所留的时钟偏差余量（AnkiConnect 可能在另一台机器上） */
const WRITE_RECONCILE_CLOCK_SLACK_MS = 10_000;

/**
 * 给某张卡片挂一条就地反馈。
 * duration > 0 时到点自动收起（触发 FluentStatusText 的退场动画）；0 表示常驻，
 * 由调用方在流程结束处 clearCardNotice。
 */
function showCardNotice(
    item: ItemModel<unknown>,
    notice: StatusNotice,
    duration = 3000,
    onAction?: () => void,
): void {
    const timer = cardNoticeTimers.get(item);
    if (timer != null) {
        window.clearTimeout(timer);
        cardNoticeTimers.delete(item);
    }
    item.notice = notice;
    if (onAction != null) {
        cardNoticeActions.set(item, onAction);
    } else {
        cardNoticeActions.delete(item);
    }
    if (duration > 0) {
        cardNoticeTimers.set(item, window.setTimeout(() => { item.notice = null; }, duration));
    }
}

/** 点击卡片提示上的操作（停止 / 重试） */
function handleCardNoticeAction(dictionary: 'collins' | 'oxford' | 'youdao', index: number): void {
    const item = wordItems[dictionary][index];
    cardNoticeActions.get(item)?.();
}

/** 收起某张卡片的就地反馈（进行中流程结束时调用） */
function clearCardNotice(item: ItemModel<unknown>): void {
    const timer = cardNoticeTimers.get(item);
    if (timer != null) {
        window.clearTimeout(timer);
        cardNoticeTimers.delete(item);
    }
    cardNoticeActions.delete(item);
    item.notice = null;
}

/** 发音失败的就地提示：挂在发音行末尾，不挤动按钮与单选项 */
const { notice: pronunciationNotice, show: showPronunciationNotice } = useTransientNotice(3000);

/** 发音按钮报告失败：就地显示 3s */
function handlePronunciationError(message: string): void {
    showPronunciationNotice(message, { severity: 'error' });
}

/**
 * 划词捕获结果：text 为录入文本（取句成功时是整句），word 为取句模式命中的单词
 * （用于预选，可能为 null），wordOffset 为该词在 text 中的 UTF-16 起始偏移（不可用时为
 * null，预选回退到按字符串匹配），retry 标记迟到的后台补取结果。
 */
interface CapturedSentencePayload {
    text: string;
    word: string | null;
    wordOffset: number | null;
    retry: boolean;
}

/** 划词面板的词元 */
const tokens = ref<{ token: string, marked: boolean; }[]>([]);
/** 所选的字典 */
const selectedDict = ref<'collins' | 'oxford' | 'youdao'>('collins');
/**
 * 搜索框文本：查询的唯一真源。只由“用户授权的输入”写入——用户手打，或用户标记手势结束时
 * 把标记词元快送进来（见 onMarksCommitted）；捕获初始化属初始状态的一部分，也直接写入
 * （见 applyCapturedSentence）。
 *
 * 反向不成立：标记状态不是 searchText 的函数变换。标记只是展示状态与“快送”手势的产物，
 * 程序化写 marked（取句预选、编辑重建分词）绝不改写查询——分词粒度与选区不一致时
 * （如只选中 encoder-decoder 里的 encoder），那个词元上的标记与查询本就允许不等价。
 */
const searchText = ref('');
/**
 * 用户是否改动过当前内容（编辑句子、输入查询、手动标记）。迟到的后台补取结果据此丢弃——
 * 补取属于同一次划词的“初始状态”，用户已经动手就不该再被覆盖。新的一次捕获会重置它。
 */
let userTouched = false;

/**
 * 本次 searchText 变化的发起方。手打时查询词是暂定值——词典跟随输入更新，AI 优选等停手；
 * 显式写入（标记快送、划词捕获）表示用户已经确认了这个词，立即连 AI 一起查
 */
let queryOrigin: 'typing' | 'explicit' = 'typing';

/**
 * 以“显式”语义写入搜索框（标记快送、划词捕获）：值真变了就走 watcher 立即查询 + AI；
 * 值没变时 watcher 不会触发，但语境可能已变（如又划到同一个词），直接按当前语境重跑一次
 */
function setQueryExplicitly(text: string): void {
    if (searchText.value === text) {
        void submitSearch();
        return;
    }
    queryOrigin = 'explicit';
    searchText.value = text;
}

/** 记一次用户改动（输入查询、编辑句子）：迟到的补取结果不再覆盖工作区 */
function onUserEdit(): void {
    userTouched = true;
}

/** 编辑句子框的输入：既算用户改动，也标记本次编辑会话确实改过句子 */
function onSentenceEdit(): void {
    userTouched = true;
    editSentenceDirty = true;
}
/** 每本字典中的搜索结果 */
const wordItems = reactive({
    'collins': [] as ItemModel<dict.CollinsItem>[],
    'oxford': [] as ItemModel<dict.OxfordItem>[],
    'youdao': [] as ItemModel<dict.YoudaoItem>[]
});
/**
 * 将有道词典的搜索结果分为 concise, web, phrase 三类。
 * 每项携带其在 wordItems.youdao 中的扁平索引：分组 v-for 的组内索引与
 * changeItemAdded/openEditDialog 按扁平索引取词的语义不一致，直接传组内索引会操作错位条目，
 * 因此卡片一律绑定 flatIndex。
 */
const wordItemsYoudao = computed(() => {
    const groups = {
        'concise': [] as { model: ItemModel<dict.YoudaoItem>, flatIndex: number }[],
        'web': [] as { model: ItemModel<dict.YoudaoItem>, flatIndex: number }[],
        'phrase': [] as { model: ItemModel<dict.YoudaoItem>, flatIndex: number }[]
    };
    wordItems.youdao.forEach((model, flatIndex) => {
        groups[model.item.meaningType].push({ model, flatIndex });
    });
    return groups;
});
/** 正在搜索或上一次成功搜索的单词，用于防止重复搜索 */
const searchingOrSearchedWords = {
    'collins': '',
    'oxford': '',
    'youdao': ''
};
/** 所选的发音 */
const selectedPronunciation = ref<'en' | 'us'>('us');

/**
 * 用户标记手势结束（单击松手、拖刷松手、Shift 区间松开）：把当前标记词元按单空格拼接后
 * 写入搜索框，等价于用户手打这段文本。这是有损的快捷输入——只送单词词元，丢原始标点与空白。
 */
function onMarksCommitted(): void {
    userTouched = true;
    setQueryExplicitly(tokens.value
        .filter(token => token.marked)
        .map(token => token.token)
        .join(' '));
}

/** SentencePanel 上报的标记写入：tokens 的写入收敛于此（整体替换与编辑重建除外）。标记不触发查询，查询只在手势结束时快送一次 */
function onMarkToken(index: number, value: boolean): void {
    userTouched = true;
    tokens.value[index].marked = value;
}

/** 所选的词典改变时，在所选词典中搜索新单词 */
watch(selectedDict, async newSelected => {
    // 上一本词典的手打查询失败提示不属于新词典的结果列表
    searchNoticeDict = null;
    clearSearchNotice();
    const succeeded = await searchAndUpdate(searchText.value, newSelected);
    // 切词典后候选总数较 AI 请求时的快照变多，说明此前预取缺货导致 AI 误走 fallback，
    // 值得用更全的候选自动补触发一次优选；候选数不再增长时不会重复触发，无循环风险
    const candidateTotal = wordItems.collins.length + wordItems.oxford.length + wordItems.youdao.length;
    if (succeeded && aiPick.phase === 'done' && aiPick.fallback
        && searchText.value.trim().length > 0 && candidateTotal > aiCandidateTotal) {
        aiPickLogic.invalidateAiPickCache(sentence.value, searchText.value);
        resetAiPick();
        void maybeStartAiPick(searchText.value);
    }
});

/** 搜索框中的文字改变时：手打走“节流词典 + 防抖 AI”，显式写入立即连 AI 一起查 */
watch(searchText, () => {
    if (queryOrigin === 'explicit') {
        queryOrigin = 'typing';
        // 显式动作优先：取消手打期间待发的 AI 防抖，避免它在新查询发起途中插队
        debouncedAiPick.cancel();
        void submitSearch();
        return;
    }
    if (selectedDict.value === 'youdao') {
        throttledYoudaoSearch();
    } else {
        throttledSearch();
    }
    debouncedAiPick(searchText.value);
});

/** 保存所选的字典 */
watch(selectedDict, newDict => {
    preference.set('selectedDict', newDict);
});

/** 保存所选的发音 */
watch(selectedPronunciation, newPronunciation => {
    preference.set('selectedPronunciation', newPronunciation);
});

/** 手打触发 AI 优选的停顿时长：业内 300–500ms 是“感知即时”的甜点，LLM 更贵故取上限 */
const AI_PICK_TYPING_DEBOUNCE_MS = 500;
/** 手打触发 AI 优选的最短查询长度：单个字符没有优选意义（显式动作不受此限） */
const AI_PICK_MIN_TYPED_LENGTH = 2;

/** 手打时只查词典（便宜、可随输入更新），节流避免过于频繁的请求 */
const throttledSearch = utils.throttle(searchDictionary, 200);

const throttledYoudaoSearch = utils.throttle(searchDictionary, 400);

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
 * 搜索单词，并用搜索结果更新 wordItems。
 *
 * 点击“查询”按钮或按下回车键时直接调用此函数。
 *
 * @param options.errorFeedback 查询失败时的反馈方式：dialog（默认，显式查询）弹错误框；
 *   inline 在结果列表上方就地提示（手打查询：逐键弹模态框会打断输入）；
 *   silent 只 console.error（AI 预取静默重试）
 * @returns 查询失败时返回 false，其余路径（含去重早退、空词清空、成功）返回 true
 */
async function searchAndUpdate(
    word: string,
    dictionary: 'collins' | 'oxford' | 'youdao',
    options?: { errorFeedback?: 'dialog' | 'inline' | 'silent'; }
): Promise<boolean> {
    word = word.trim();
    if (word.length === 0) {
        wordItems[dictionary] = [];
        // 界面上的单词列表已清空，无法复用，因此对于任何单词都要重新搜索
        searchingOrSearchedWords[dictionary] = '';
        clearSearchNoticeOf(dictionary);
        return true;
    }
    if (word === searchingOrSearchedWords[dictionary]) {
        // 无需重复搜索
        // - 若 word 正在被搜索，则等待搜索结果即可
        // - 若 word 上次已被搜索成功，则复用界面上的单词列表
        return true;
    }
    // 新单词的搜索请求，清空界面上的已有的搜索结果
    wordItems[dictionary] = [];
    // 函数体此前无 await，因此此处更新 searchingOrSearchedWords 遵循 searchAndUpdate 的调用顺序
    // 即，后续的 searchAndUpdate 调用会覆盖前面的 searchingOrSearchedWords
    searchingOrSearchedWords[dictionary] = word;
    let results: dict.CollinsItem[] | dict.OxfordItem[] | dict.YoudaoItem[];
    try {
        if (dictionary === 'collins') {
            results = await dict.searchCollins(word);
        } else if (dictionary === 'oxford') {
            results = await dict.searchOxford(word);
        } else if (dictionary === 'youdao') {
            results = await dict.searchYoudao(word);
        } else {
            throw new Error(`Unknown dictionary: ${String(dictionary)}`);
        }
        // 由于每次搜索的网络延迟不等长，await 后控制流可能晚于后续的 searchAndUpdate调用，那么本次搜索结果将无效
        // 根据 searchingOrSearchedWords[dictionary] 是否被覆盖即可判断本次搜索结果是否仍然有效
    } catch (error) {
        // 仅在本次搜索单词与当前 searchingOrSearchedWords 相同时才显示错误
        // 否则，说明存在新的 searchAndUpdate 调用（它覆盖了 searchingOrSearchedWords），本次搜索结果无效
        if (searchingOrSearchedWords[dictionary] === word) {
            searchingOrSearchedWords[dictionary] = '';
            console.error(error);
            const feedback = options?.errorFeedback ?? 'dialog';
            if (feedback === 'inline') {
                showSearchNotice(`查询失败：${error instanceof Error ? error.message : String(error)}`, {
                    severity: 'error',
                    duration: 0,
                });
                searchNoticeDict = dictionary;
            } else if (feedback === 'dialog') {
                await dialog.message(String(error), { title: '查询失败', kind: 'error' });
            }
        }
        return false;
    }
    // 仅在本次搜索结果有效时更新搜索结果单词列表
    if (searchingOrSearchedWords[dictionary] === word) {
        wordItems[dictionary] = results.map(item => ({ item, status: 'not-added', id: null }) as ItemModel<any>);
        clearSearchNoticeOf(dictionary);
    }
    return true;
}

/**
 * 手打查询失败的就地提示（显示在结果列表上方）。常驻到该词典下一次查询成功、
 * 查询词被清空或切换词典为止：网络持续故障时边打字边失败，提示保持展开只更新文案，不反复伸缩
 */
const { notice: searchNotice, show: showSearchNotice, clear: clearSearchNotice } = useTransientNotice(0);
/** 当前就地提示属于哪本词典：别的词典查询成功不应收起它 */
let searchNoticeDict: 'collins' | 'oxford' | 'youdao' | null = null;

function clearSearchNoticeOf(dictionary: 'collins' | 'oxford' | 'youdao'): void {
    if (searchNoticeDict === dictionary) {
        searchNoticeDict = null;
        clearSearchNotice();
    }
}
// #endregion

// #region 文本框句子编辑
/** 文本框中的句子 */
const sentence = ref('');
/** 是否显示文本框 */
const showEdit = ref(false);
/** 文本框控件（聚焦经组件暴露的 focus 方法转发到内部 textarea） */
const editTextArea = ref<{ focus: () => void } | null>(null);
const isMacOS = computed(() => api.os.type() === 'macos');

/** 根据平台显示不同的快捷键提示 */
const editPlaceholder = computed(() => {
    return isMacOS.value
        ? '⌘ + Enter 完成编辑'
        : 'Ctrl + Enter 完成编辑';
});

/**
 * 编辑会话的基线分词：进入编辑模式时快照，确认编辑时以此为准重建分词。
 *
 * 编辑框中的内容视为草稿：编辑期间逐键输入不重建分词（避免把“拆开单词”的
 * 中间状态固化下来，导致复原后标记无从恢复），确认（完成）时才一次性以基线
 * 为基准重建，尽量保留已标记单词。整体替换句子（划词录入、粘贴）会作废草稿。
 */
let editBaselineTokens: { token: string; marked: boolean; }[] | null = null;
/**
 * 本次编辑会话中句子是否被改动过：确认编辑时据此决定要不要按新语境重取释义。
 * 用“改过”而不是“前后文本不同”——改完又改回去时文本相同，但旧释义已被清空，仍应恢复
 */
let editSentenceDirty = false;

/**
 * 程序化整体替换句子（划词录入、粘贴、dev 测试句等）：
 * 句子与分词同步重建为全新状态（清除已有标记），并作废进行中的编辑草稿。
 * 值相同也重建——每次整体替换都应是全新状态。
 */
function replaceSentence(text: string): void {
    editBaselineTokens = null;
    editSentenceDirty = false;
    if (sentence.value !== text) {
        sentence.value = text;
    }
    tokens.value = utils.string.tokenize(text).map(token => ({ token, marked: false }));
}

/** 粘贴失败的就地提示：显示在句子面板上方，紧挨着“粘贴”按钮 */
const { notice: pasteNotice, show: showPasteNotice } = useTransientNotice(3000);

async function pasteToEdit() {
    let text: string;
    try {
        text = await api.clipboard.readText();
    } catch (error) {
        // 剪贴板为空或内容不是文本（如图片）时，剪贴板插件直接报错而不是返回空串
        console.error(error);
        showPasteNotice('剪贴板中没有文本', { severity: 'error' });
        return;
    }
    const trimmed = text.trim();
    if (trimmed.length === 0) {
        // 只有空白：不替换句子，避免一次误粘贴清掉当前工作区
        showPasteNotice('剪贴板中没有文本', { severity: 'error' });
        return;
    }
    // 粘贴属于整体替换句子：分词同步重建为全新状态，不保留旧的标记，编辑草稿一并作废；
    // 旧句子的查询随之作废（查询不由标记派生，需显式清空）
    replaceSentence(trimmed);
    setQueryExplicitly('');
    userTouched = true;
    if (showEdit.value) {
        await changeEditStatus();
    }
}

async function changeEditStatus() {
    showEdit.value = !showEdit.value;
    if (showEdit.value) {
        // 进入编辑模式：快照当前分词作为基线，编辑期间不再重建分词；清空“本次编辑是否改过句子”
        editBaselineTokens = tokens.value.map(({ token, marked }) => ({ token, marked }));
        editSentenceDirty = false;
        await nextTick();
        editTextArea.value?.focus();
    } else {
        // 首尾空白在确认时才裁剪：编辑期间逐键裁剪会吞掉句末刚打的空格（见 fluent-controls/useTextModel.ts）
        const trimmed = sentence.value.trim();
        if (trimmed !== sentence.value) {
            sentence.value = trimmed;
        }
        // 确认编辑（完成）：以基线分词为基准重建，尽量保留已标记单词。
        // 基线为 null 说明编辑期间句子被整体替换（划词录入、粘贴），分词已是全新状态，无需重建
        if (editBaselineTokens != null) {
            tokens.value = utils.string.rebuildTokensPreservingMarks(editBaselineTokens, sentence.value);
            editBaselineTokens = null;
            // 句子改过 = 语境变了：按同一个查询词重新取释义。查询由用户书写，编辑句子不改写它，
            // 但 AI 优选的缓存 key 含句子，会据此重新请求（同句同词命中结果缓存，不重复拉取）。
            // 没改动过则什么都不做，避免白白消耗一次 LLM 请求
            if (editSentenceDirty && searchText.value.trim().length > 0) {
                void submitSearch();
            }
        }
        editSentenceDirty = false;
    }
}

/** 处理键盘事件，支持 macOS 的 Cmd + Enter 和其他平台的 Ctrl + Enter */
async function handleEditTextAreaKeydown(event: KeyboardEvent) {
    const isCorrectModifier = isMacOS.value ? event.metaKey : event.ctrlKey;
    if (isCorrectModifier && event.key === 'Enter') {
        event.preventDefault();
        await changeEditStatus();
    }
}
// #endregion

// #region AI 优选释义
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
function isAiPicked(source: aiPickLogic.DictSource, index: number): boolean {
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

/** AI 优选卡片上提示的操作（停止 / 重试）：转发给源条目 */
function handleAiPickNoticeAction(): void {
    if (aiPick.pick != null) {
        handleCardNoticeAction(aiPick.pick.source, aiPick.pick.index);
    }
}

/** AI 优选命中的词典条目（供 AiPickCard 渲染完整内容）；pick 为空或条目缺失时为 null */
const aiPickItem = computed(() => {
    if (aiPick.pick == null) return null;
    const item = wordItems[aiPick.pick.source][aiPick.pick.index]?.item;
    return item != null ? { source: aiPick.pick.source, item } : null;
});

/**
 * 提交一次有效搜索后调用：若启用并配置好了 LLM，则并行搜索三本词典（填满 wordItems，
 * 保证候选与界面条目索引对齐、切 tab 即时）并发起 AI 优选；否则中止遗留请求并重置为 idle。
 */
async function maybeStartAiPick(word: string) {
    // 显式发起（或本次防抖已到期）时取消待发的防抖，避免停手计时到期再发一次
    debouncedAiPick.cancel();
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
        const prefetch = async (dictionary: 'collins' | 'oxford' | 'youdao'): Promise<void> => {
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
 * 只查当前词典，不触发 AI 优选：手打路径专用（AI 由 debouncedAiPick 另行发起）。
 * 失败只就地提示：网络故障时每个节流后的查询都会失败，弹模态框会排队弹出并抢走输入焦点；
 * 回车或点“查询”时才弹框
 */
async function searchDictionary(): Promise<void> {
    await searchAndUpdate(searchText.value, selectedDict.value, { errorFeedback: 'inline' });
}

/** 提交一次有效搜索：更新当前词典的搜索结果并立即触发 AI 优选（回车、查询按钮等显式动作） */
async function submitSearch() {
    await searchAndUpdate(searchText.value, selectedDict.value);
    void maybeStartAiPick(searchText.value);
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
// #endregion

// #region 全局快捷键划词录入
/**
 * 后台仍在补取完整句子（Rust 侧重试在途）：句子面板角上的指示环据此显示/收束。
 * 补取结果会覆盖先到的降级结果（仅词），指示环只是让“还会变”这件事有个连续、安静的信号。
 */
const captureSettling = ref(false);

/**
 * 录入捕获的句子：整体替换当前句子（分词同步重建为全新状态）并退出编辑模式。
 * 取句命中的单词作为查询直接写入搜索框，同时在分词结果中尽力预选（位置提示）
 */
async function applyCapturedSentence(payload: CapturedSentencePayload) {
    const trimmed = payload.text.trim();
    if (trimmed.length === 0) {
        return;
    }
    // 迟到的补取结果（同一句话的后台重试）只在用户未改动时生效：用户已经编辑、输入或
    // 动手标记，说明工作区已被接管，补取不得覆盖
    if (payload.retry && userTouched) {
        console.log('[capture] dropping the late retry result: the user has already edited the workspace');
        return;
    }
    // 捕获来源日志（console 会转发到应用日志文件）：定位“错句覆盖”类问题时
    // 区分事件/暂存回放，并与 Rust 侧的 capture 日志对照
    console.log(
        `[capture] applying: word=${JSON.stringify(payload.word)}, retry=${payload.retry}, text=${JSON.stringify(trimmed.slice(0, 80))}`
    );
    // 按快捷键取词时应用可能停留在设置页：切回主界面，避免句子已被替换但用户看到的仍是设置页；
    // 离开前立即落盘设置页可能还未防抖保存的修改（与设置页返回按钮行为一致）
    if (router.currentRoute.value.path !== '/') {
        await useSettingsStore().flush();
        await router.push('/');
    }
    // 先整体替换：分词同步重建为全新状态（每次捕获都是全新状态），进行中的编辑草稿一并作废
    replaceSentence(trimmed);
    if (showEdit.value) {
        await changeEditStatus();
    }
    const word = payload.word?.trim();
    if (word != null && word.length > 0) {
        // 捕获词是查询的权威来源，直接写入搜索框；分词面板的预选只是位置提示，
        // 分词粒度与选区不一致时（如选中的是 encoder-decoder 里的 encoder）
        // 标记会落在整个复合词元上，与查询不等价是允许的
        setQueryExplicitly(word);
        // 偏移以载荷原文为基准：文本被 trim 改短时偏移不再可信
        const wordOffset = trimmed.length === payload.text.length ? payload.wordOffset : null;
        // replaceSentence 已同步重建分词，可直接标记命中的单词
        markCapturedWord(word, wordOffset, trimmed);
    } else {
        // 未命中单词（如手动跨句选择）：句子已整体替换，旧查询一并清空
        setQueryExplicitly('');
    }
    userTouched = false;
}

/**
 * 在分词结果中尽力标记捕获的单词。首选按选区偏移标记相交词元（同一单词在句中多次出现
 * 时也能标中真正被选中的那一处），偏移缺失或与文本不符时退回字符串匹配：
 * 精确词元 → 忽略大小写 → 连续词元短语 → 复合词元内的子串（encoder 落在
 * encoder-decoder 这个词元上）。
 *
 * 都不匹配就不标记：标记只是位置提示，缺位不影响查询——捕获词已经直接写进搜索框。
 */
function markCapturedWord(word: string, wordOffset: number | null, sentence: string): void {
    const list = tokens.value;
    const lower = word.toLowerCase();
    if (wordOffset != null && markTokensInRange(list, sentence, wordOffset, word)) {
        return;
    }
    // 单词匹配：精确，失败再忽略大小写
    let index = list.findIndex(t => t.token === word);
    if (index < 0) {
        index = list.findIndex(t => t.token.toLowerCase() === lower);
    }
    if (index >= 0) {
        list[index].marked = true;
        return;
    }
    // 短语匹配：按空白切分后，在词元下标序列上找连续的一段与各部分忽略大小写相等
    const parts = word.split(/\s+/).filter(part => part.length > 0);
    if (parts.length > 1) {
        const wordIndexes = list
            .map((t, i) => ({ token: t.token, index: i }))
            .filter(t => utils.string.isWord(t.token))
            .map(t => t.index);
        outer: for (let start = 0; start + parts.length <= wordIndexes.length; start++) {
            for (let offset = 0; offset < parts.length; offset++) {
                if (list[wordIndexes[start + offset]].token.toLowerCase() !== parts[offset].toLowerCase()) {
                    continue outer;
                }
            }
            for (let offset = 0; offset < parts.length; offset++) {
                list[wordIndexes[start + offset]].marked = true;
            }
            return;
        }
    }
    // 复合词元内的子串选区：分词把词内连字符与撇号并入词元（encoder-decoder、don't 是
    // 单个词元，见 stringutils.ts），而取词可能只选中其中一段。按词元内的连接符切段后
    // 整段相等才命中（不做任意子串匹配，避免 code 命中 encoder），命中则标记整个词元
    const partIndex = list.findIndex(t => t.token.toLowerCase().split(/['’-]/).includes(lower));
    if (partIndex >= 0) {
        list[partIndex].marked = true;
    }
}

/**
 * 按选区区间标记词元：标记所有与 [offset, offset + word.length) 相交的单词词元——选区
 * 可能只是复合词元的一部分（encoder 在 encoder-decoder 内），也可能横跨多个词元。
 * 偏移与文本不一致（被裁剪或改写）时返回 false，由调用方回退到字符串匹配。
 */
function markTokensInRange(
    list: { token: string, marked: boolean; }[],
    sentence: string,
    offset: number,
    word: string
): boolean {
    const end = offset + word.length;
    if (offset < 0 || end > sentence.length) {
        return false;
    }
    if (sentence.slice(offset, end).toLowerCase() !== word.toLowerCase()) {
        return false;
    }
    let position = 0;
    let marked = false;
    for (const item of list) {
        const start = position;
        position += item.token.length;
        if (position <= offset || start >= end) {
            continue;
        }
        if (utils.string.isWord(item.token)) {
            item.marked = true;
            marked = true;
        }
    }
    return marked;
}

/**
 * 划词没有录入内容的原因（与后端 CaptureFailure 对应）：
 * empty = 目标应用里没有可取用的选区；failed = 取词过程报错
 */
type CaptureFailure = 'empty' | 'failed';

/** 弹出划词未录入内容的提示（按失败种类与平台给出引导） */
function showCaptureFailureDialog(failure: CaptureFailure) {
    // 两种提示都以操作失败处理（ContentDialog 的 kind 只有 error/warning 两级，
    // 而 warning 是给破坏性操作确认用的）。
    //
    // empty：目标应用里没有可取用的选区——真的没选中文字，或应用不向无障碍接口
    // 暴露选区（Windows 上部分 UWP 应用如此；macOS 上如 VSCode 的编辑器）。
    // 托盘菜单路径下的取词目标是“用户最近所在的应用窗口”，所以还要提示先点回目标应用。
    //
    // failed：无障碍查询与模拟复制都没拿到内容——Windows 上常见于以管理员身份
    // 运行的窗口（UIPI 拦截注入）；macOS 上多为辅助功能未授权；Linux 上为 AT-SPI
    // 未暴露选区且 PRIMARY 也为空。
    const isWindows = api.os.type() === 'windows';
    let message: string;
    if (failure === 'empty') {
        message = isMacOS.value
            ? '取词目标里没有可取用的选区：可能没有选中文字，也可能该应用不向辅助功能接口暴露选区（如 VSCode 的编辑器）。请选中文字后重试。'
            : isWindows
                ? '取词目标里没有可取用的选区：可能没有选中文字，也可能该应用不向无障碍接口暴露选区（部分 UWP 应用如此）。\n\n从托盘菜单触发时，取词目标是“你最近所在的应用窗口”，请先点击要取词的应用窗口再重试。'
                : '取词目标里没有可取用的选区：可能没有选中文字，也可能该应用未通过 AT-SPI 暴露选区；GNOME Wayland 下也不支持读取 PRIMARY 选区。请选中文字后重试。';
    } else {
        message = isMacOS.value
            ? '获取选中文本失败。\n\n请在“系统设置 → 隐私与安全性 → 辅助功能”中允许本应用，然后重试。'
            : isWindows
                ? '获取选中文本失败。\n\n目标应用既没有通过 UI Automation 暴露选区，也没有响应模拟的 Ctrl+C 复制（常见于以管理员身份运行的窗口，请换用普通权限的窗口重试）。若从托盘菜单触发，请先点击目标应用窗口再重试。'
                : '获取选中文本失败。\n\n目标应用没有通过 AT-SPI 暴露选区，也没能读到 PRIMARY 选区（请确认已选中文本；GNOME Wayland 下不支持读取 PRIMARY）。';
    }
    const title = failure === 'empty' ? '没有取到文本' : '划词录入失败';
    void dialog.message(message, { title, kind: 'error' });
}

/** 监听划词句子事件，并取走可能在主窗口重建期间暂存的句子 */
async function initSentenceCapture() {
    // 后台补取状态：先挂监听、再主动查一次。重试在窗口显示之前就已启动，窗口被重建时
    // “开始”事件必然丢失，只能靠 is_capture_settling 查回；若挂监听后已收到过事件，
    // 则以事件为准（避免被这次查询返回的稍旧值回退）
    let settlingEventReceived = false;
    try {
        await api.event.listen<boolean>('sentence-capture-settling', event => {
            settlingEventReceived = true;
            captureSettling.value = event.payload;
        });
        if (!settlingEventReceived) {
            captureSettling.value = await utils.invoke<boolean>('is_capture_settling');
        }
    } catch (error) {
        console.error(error);
    }
    // 主窗口被关闭后按快捷键会重建窗口，前端就绪之前 emit 的事件会丢失，
    // 此时通过 take_pending_sentence 取回 Rust 侧暂存的句子。
    //
    // 后端每次捕获都会暂存一份，事件送达后也要取走丢弃：否则暂存一直留着，页面一旦重载
    // （WebView 重载、内容进程崩溃）就会把旧句子回放一遍。
    // 事件若恰好落在“挂监听”与“取暂存”之间，同一个结果会经两条路各到一次：
    // 记下从暂存回放的那份，挂监听后收到的第一个事件与它相同就跳过
    let sentenceEventSeen = false;
    let replayedSentence: CapturedSentencePayload | null = null;
    try {
        await api.event.listen<CapturedSentencePayload>('sentence-captured', event => {
            discardPending('take_pending_sentence');
            const replayed = replayedSentence;
            replayedSentence = null;
            const isFirstEvent = !sentenceEventSeen;
            sentenceEventSeen = true;
            if (isFirstEvent && replayed != null && isSameCapturedPayload(replayed, event.payload)) {
                return;
            }
            void applyCapturedSentence(event.payload);
        });
        const pending = await utils.invoke<CapturedSentencePayload | null>('take_pending_sentence');
        // 已经收到过事件时，暂存里的不会比它新（暂存只保留最近一次），交给事件即可
        if (pending != null && !sentenceEventSeen) {
            replayedSentence = pending;
            await applyCapturedSentence(pending);
        }
    } catch (error) {
        console.error(error);
    }
    let failureEventSeen = false;
    let replayedFailure: CaptureFailure | null = null;
    try {
        await api.event.listen<CaptureFailure>('sentence-capture-failed', event => {
            discardPending('take_pending_capture_failure');
            const replayed = replayedFailure;
            replayedFailure = null;
            const isFirstEvent = !failureEventSeen;
            failureEventSeen = true;
            if (isFirstEvent && replayed === event.payload) {
                return;
            }
            showCaptureFailureDialog(event.payload);
        });
        // 划词未录入内容时会弹出主窗口并提示；若前端当时尚未就绪，事件已丢失，
        // 此处取回暂存的失败原因补弹提示
        const pendingFailure = await utils.invoke<CaptureFailure | null>('take_pending_capture_failure');
        if (pendingFailure != null && !failureEventSeen) {
            replayedFailure = pendingFailure;
            showCaptureFailureDialog(pendingFailure);
        }
    } catch (error) {
        console.error(error);
    }
}

/** 结果已经由事件送达：取走后端的暂存并丢弃（失败只记日志，不影响本次结果的应用） */
function discardPending(command: 'take_pending_sentence' | 'take_pending_capture_failure'): void {
    utils.invoke<unknown>(command).catch((error: unknown) => {
        console.error(error);
    });
}

function isSameCapturedPayload(a: CapturedSentencePayload, b: CapturedSentencePayload): boolean {
    return a.text === b.text && a.word === b.word && a.wordOffset === b.wordOffset && a.retry === b.retry;
}
// #endregion

// #region Anki
/** 若牌组或笔记模板不存在，则创建之（signal 仅用于“已取消时不再弹错误框”） */
async function prepareDeckAndModel(deckName: string, modelName: string, signal?: AbortSignal) {
    let errorTitle: string | null = null;

    async function prepareDeck(deckName: string) {
        try {
            await ankiService.createDeck(deckName);
        } catch (error) {
            errorTitle = `牌组 ${deckName} 创建失败`;
            throw error;
        }
    }

    /**
     * @returns 是否创建了新的笔记模板
     */
    async function prepareModel(modelName: string): Promise<boolean> {
        let modelExists: boolean;
        try {
            const modelNames = await ankiService.modelNames();
            modelExists = modelNames.includes(modelName);
        } catch (error) {
            errorTitle = `查询笔记模板 ${modelName} 失败`;
            throw error;
        }
        if (!modelExists) {
            try {
                await ankiService.createMarkerModel(modelName);
            } catch (error) {
                errorTitle = `笔记模板 ${modelName} 创建失败`;
                throw error;
            }
            return true;
        }
        return false;
    }

    let newModelCreated: boolean;
    try {
        newModelCreated = (await Promise.all([prepareDeck(deckName), prepareModel(modelName)]))[1];
    } catch (error) {
        // 用户点了“停止”：这是预期内的中断，不再弹错误框
        if (signal?.aborted !== true) {
            await dialog.message(String(error), { title: errorTitle!, kind: 'error' });
        }
        throw error;
    }
    if (newModelCreated) {
        globals.templateVersion.value = anki.CARD_TEMPLATE_VERSION;
    } else {
        // 若模板版本号未获取，则此时尝试获取一次
        if (typeof globals.templateVersion.value !== 'string') {
            void globals.fetchAndSetTemplateVersion(modelName);
        }
    }
}

/**
 * 一次加卡/移除操作的取消句柄。
 *
 * - 前 700ms 不显示任何提示：正常速度的添加/移除不应闪一下；
 * - 超过阈值才出现“进度 + 停止”，点停止后界面立即退出等待；
 * - signal 交给可在 HTTP 层中断的请求（删除）；无法中断的等待（共享的 Anki 启动、
 *   准备牌组）用 race 退出——共享启动不会因为某一张卡片停止而被打断，只解除本卡片的等待；
 * - disableStop 之后不再提供“停止”（写入笔记阶段，见 changeItemAddedOf）。
 */
function createCardOperation(item: ItemModel<unknown>, initialMessage: string) {
    const controller = new AbortController();
    let rejectCancelled: ((reason?: unknown) => void) | null = null;
    const cancelledPromise = new Promise<never>((_, reject) => { rejectCancelled = reject; });
    let stopped = false;
    let stoppable = true;
    let noticeVisible = false;
    let slowTimer: number | null = null;

    const clearSlowTimer = (): void => {
        if (slowTimer != null) {
            window.clearTimeout(slowTimer);
            slowTimer = null;
        }
    };

    const stop = (): void => {
        if (stopped || !stoppable) {
            return;
        }
        stopped = true;
        clearSlowTimer();
        controller.abort();
        rejectCancelled?.(new Error('操作已停止'));
    };

    const showProgress = (message: string): void => {
        if (stopped) {
            return;
        }
        clearSlowTimer();
        noticeVisible = true;
        if (stoppable) {
            showCardNotice(item, { severity: 'informational', message, progress: true, actionLabel: '停止' }, 0, stop);
        } else {
            showCardNotice(item, { severity: 'informational', message, progress: true }, 0);
        }
    };

    /** 仅在提示已经可见时更新文案：避免快操作因为一句“正在写入…”闪一下 */
    const updateProgress = (message: string): void => {
        if (noticeVisible) {
            showProgress(message);
        }
    };

    /**
     * 进入不可中止的阶段，并（在提示已可见时）把文案换成 message、去掉“停止”。
     * 已经停止时返回 false，调用方不应再继续
     */
    const disableStop = (message: string): boolean => {
        if (stopped) {
            return false;
        }
        stoppable = false;
        updateProgress(message);
        return true;
    };

    slowTimer = window.setTimeout(() => { slowTimer = null; showProgress(initialMessage); }, SLOW_NOTICE_DELAY_MS);

    return {
        signal: controller.signal,
        get stopped() { return stopped; },
        race: <T>(promise: Promise<T>): Promise<T> => Promise.race([promise, cancelledPromise]),
        showProgress,
        updateProgress,
        disableStop,
        dispose: clearSlowTimer,
    };
}

/**
 * 切换指定词典中指定条目的加卡状态（添加/取消添加）。
 *
 * 词典参数独立出来，是为了让 AI 优选卡片能跨词典操作其选中的条目（可能与当前 tab 不同）；
 * AI 笔记注入判断仍用 isAiPicked(dictionary, index)，仅命中条目带笔记时写入“笔记”字段
 */
async function changeItemAddedOf(dictionary: 'collins' | 'oxford' | 'youdao', index: number) {
    const item = wordItems[dictionary][index];
    const pronunciationType = selectedPronunciation.value;
    const deckName = cfg.effectiveTextSetting('deckName', config.deckName);
    const modelName = cfg.effectiveTextSetting('modelName', config.modelName);
    // 上一次操作留下的提示（含失败提示）在这里清掉，避免与新一次操作叠加
    clearCardNotice(item);
    if (item.status === 'not-added') { // add to Anki
        // 写入内容在第一个 await 之前快照：之后可能要等 Anki 启动几十秒，期间用户可能又划了一句、
        // 编辑了句子，或得到了新词的 AI 优选（恰好落在同一词典的同一序号上），这些都不能混进这张卡片。
        // AI 笔记只取已完成的优选：流式输出中的笔记是半截的，提前解析出的 pick 也可能被最终结果推翻
        const sentenceHTML = makeSentenceHTML();
        const aiNote = (aiPick.phase === 'done' && isAiPicked(dictionary, index)) ? aiPick.note : null;
        item.status = 'processing-add';
        const operation = createCardOperation(item, '正在添加到 Anki…');
        // 出错或中断时先清掉进行中的提示，再决定显示什么（成功则什么都不显示）
        const settle = () => {
            operation.dispose();
            clearCardNotice(item);
        };
        let phase: 'connect' | 'prepare' | 'write' = 'connect';
        /** 写入请求是否已经发出：发出之后的失败不能直接当作“未添加” */
        let noteSent = false;
        let fields: ReturnType<typeof anki.makeFields> | null = null;
        let writeStartedAt = 0;
        try {
            // 确保 AnkiConnect 可用：不可用时按配置自动启动 Anki 并等待其就绪
            await operation.race(globals.ensureAnkiConnect(
                message => operation.showProgress(message),
                { signal: operation.signal },
            ));
            // 连接已就绪：文案切到“添加中”，不要再停在“启动/连接 Anki”
            operation.updateProgress('正在添加到 Anki…');
            phase = 'prepare';
            await operation.race(prepareDeckAndModel(deckName, modelName, operation.signal));
            phase = 'write';
            // AI 优选联动：若所加条目正是 AI 优选命中的条目且带有笔记，则写入“笔记”字段
            fields = anki.makeFields(dictionary, item.item, sentenceHTML, aiNote);
            const word = ('phrase' in item.item && item.item.phrase != null)
                ? item.item.phrase
                : item.item.word;
            // 发音地址全部不可用时不附带音频（见 addMarkerNote 的说明），不再把已判定不可用的地址交给 AnkiConnect
            const audioResult = await operation.race(dict.makePronunciationURL(word, pronunciationType));
            const audio = audioResult != null
                ? {
                    url: audioResult.url,
                    filename: await dict.makePronunciationFilename(word, pronunciationType, audioResult.dict),
                }
                : null;
            // 写入请求一旦发出，Anki 就会照常完成（含下载音频），前端放弃等待并不能撤回：
            // 从这里起不再提供“停止”，也不把请求挂在可中止的信号上
            if (!operation.disableStop('正在写入笔记…')) {
                throw new Error('操作已停止');
            }
            // 笔记 id 是创建时刻的毫秒时间戳，核对时据此只看本次写入之后创建的笔记；
            // 留出余量，容忍 AnkiConnect 在另一台机器上时的时钟偏差
            writeStartedAt = Date.now() - WRITE_RECONCILE_CLOCK_SLACK_MS;
            noteSent = true;
            const id = await ankiService.addMarkerNote(deckName, modelName, fields, audio);
            if (id == null) {
                throw new Error('addNote returns null');
            }
            item.id = id;
            item.status = 'is-added';
            settle();
        } catch (error) {
            const stopped = operation.stopped;
            operation.dispose();
            if (stopped) {
                // 用户主动停止：回到未添加，给一句短提示说明“确实停了”
                item.status = 'not-added';
                clearCardNotice(item);
                showCardNotice(item, { severity: 'informational', message: '已停止添加' }, 1500);
                return;
            }
            console.error(error);
            if (phase === 'connect') {
                item.status = 'not-added';
                clearCardNotice(item);
                await dialog.message(String(error), { title: '无法连接 Anki', kind: 'error' });
            } else if (phase === 'write') {
                if (noteSent && fields != null) {
                    await settleFailedWrite(item, error, { deckName, modelName, fields, writeStartedAt },
                        () => { void changeItemAddedOf(dictionary, index); });
                    return;
                }
                // 写入类失败通常是偶发的，就地提示并给出重试，不打断连续加卡
                item.status = 'not-added';
                clearCardNotice(item);
                showCardNotice(item, {
                    severity: 'error',
                    message: `添加失败：${error instanceof Error ? error.message : String(error)}`,
                    actionLabel: '重试',
                }, 8000, () => { void changeItemAddedOf(dictionary, index); });
            } else {
                // 准备牌组/模板：错误框由 prepareDeckAndModel 自己弹出
                item.status = 'not-added';
                clearCardNotice(item);
            }
        }
    } else if (item.status === 'is-added') { // remove from Anki
        item.status = 'processing-remove';
        const operation = createCardOperation(item, '正在从 Anki 移除…');
        try {
            await operation.race(ankiService.deleteNotes([item.id!], operation.signal));
            item.id = null;
            item.status = 'not-added';
            operation.dispose();
            clearCardNotice(item);
        } catch (error) {
            operation.dispose();
            if (operation.stopped) {
                item.status = 'is-added';
                clearCardNotice(item);
                showCardNotice(item, { severity: 'informational', message: '已停止移除' }, 1500);
                return;
            }
            item.status = 'is-added';
            console.error(error);
            clearCardNotice(item);
            showCardNotice(item, {
                severity: 'error',
                message: `移除失败：${error instanceof Error ? error.message : String(error)}`,
                actionLabel: '重试',
            }, 8000, () => { void changeItemAddedOf(dictionary, index); });
        }
    }
}

/**
 * 写入请求已发出却报错（超时、连接中断、AnkiConnect 报错）时决定卡片状态。
 *
 * 前端没等到响应不代表 Anki 没写入：先向 Anki 核对一次，确实写入了就按已添加处理（并接上笔记 id，
 * 之后仍能移除）；确认没有才回到未添加并给出重试；连核对都失败时状态未知，回到未添加但不给“重试”，
 * 提示用户先去 Anki 里确认，避免重复添加
 */
async function settleFailedWrite(
    item: ItemModel<unknown>,
    error: unknown,
    write: { deckName: string; modelName: string; fields: ReturnType<typeof anki.makeFields>; writeStartedAt: number; },
    retry: () => void,
): Promise<void> {
    showCardNotice(item, { severity: 'informational', message: '正在核对添加结果…', progress: true }, 0);
    let foundId: number | null = null;
    let reconcileFailed = false;
    try {
        foundId = await ankiService.findAddedMarkerNote(write.deckName, write.modelName, write.fields, write.writeStartedAt);
    } catch (reconcileError) {
        console.error(reconcileError);
        reconcileFailed = true;
    }
    clearCardNotice(item);
    const message = error instanceof Error ? error.message : String(error);
    if (foundId != null) {
        console.warn(`[anki] addNote 报错，但核对发现笔记已写入（id=${foundId}），按已添加处理`);
        item.id = foundId;
        item.status = 'is-added';
        return;
    }
    item.status = 'not-added';
    if (reconcileFailed) {
        showCardNotice(item, {
            severity: 'warning',
            message: `无法确认是否已添加（${message}）。请先在 Anki 中查看，避免重复添加。`,
        }, 10000);
        return;
    }
    showCardNotice(item, { severity: 'error', message: `添加失败：${message}`, actionLabel: '重试' }, 8000, retry);
}

/** 词典卡片的加卡入口：操作当前所选词典中的条目 */
async function changeItemAdded(index: number) {
    await changeItemAddedOf(selectedDict.value, index);
}

/** AI 优选卡片的加卡入口：操作 AI 选中的那条真实词典条目，可能与当前 tab 不同词典 */
async function handleAiPickAdd() {
    if (aiPick.pick != null) {
        await changeItemAddedOf(aiPick.pick.source, aiPick.pick.index);
    }
}

/**
 * 点击单词卡片的“编辑笔记”按钮后，打开 Anki 的编辑对话框。
 *
 * Anki 26.08 重构了编辑窗口（移除了 Ui_Dialog 的 buttonBox），导致 AnkiConnect 的
 * guiEditNote 报错。
 *
 * 因此在 guiEditNote 失败时降级为打开 Anki 卡片浏览器并定位到该笔记，卡片浏览器中可直接编辑。
 */
async function openEditDialogOf(dictionary: 'collins' | 'oxford' | 'youdao', index: number) {
    const item = wordItems[dictionary][index];
    if (item.id == null) {
        console.error('item.id is null when openEditDialog, item:', item);
        return;
    }
    try {
        await ankiService.guiEditNote(item.id);
    } catch (error) {
        console.error(error);
        try {
            await ankiService.guiBrowse(`nid:${item.id}`);
            // 降级结果就写在触发的这张卡片里；文案较长，多留一点阅读时间
            showCardNotice(item, {
                severity: 'warning',
                message: '编辑对话框打开失败（Anki 26.08 与 AnkiConnect 不兼容），已在 Anki 卡片浏览器中定位该笔记，选中后可在右侧直接编辑。',
            }, 8000);
        } catch (fallbackError) {
            console.error(fallbackError);
            await dialog.message(String(error), { title: '打开编辑对话框失败', kind: 'error' });
        }
    }
}

/** 词典卡片的编辑入口：操作当前所选词典中的条目 */
async function openEditDialog(index: number) {
    await openEditDialogOf(selectedDict.value, index);
}

/** AI 优选卡片的编辑入口：操作 AI 选中的那条真实词典条目，可能与当前 tab 不同词典 */
async function handleAiPickEdit() {
    if (aiPick.pick != null) {
        await openEditDialogOf(aiPick.pick.source, aiPick.pick.index);
    }
}

/**
 * 生成卡片例句的 HTML（标记的词加粗）。
 *
 * 编辑中还没点“完成”时，用户看到的是编辑框里的草稿，而 tokens 在编辑期间刻意不重建、仍是编辑前的句子：
 * 此时按确认编辑的同一规则从草稿生成分词，加卡内容与用户所见一致，且不打断编辑
 */
function makeSentenceHTML(): string {
    const source = (showEdit.value && editBaselineTokens != null)
        ? utils.string.rebuildTokensPreservingMarks(editBaselineTokens, sentence.value.trim())
        : tokens.value;
    return source.map(
        ({ token, marked }) => marked
            ? `<b>${utils.string.escapeHTML(token)}</b>`
            : utils.string.escapeHTML(token)
    ).join('');
}
// #endregion

// 由于使用了 KeepAlive 不销毁页面，所以只会执行一次
onBeforeMount(async () => {
    // 为需要初始化的变量赋值
    await globals.initAtAppStart();
    [config, ankiService] = await Promise.all([
        globals.getConfig(),
        globals.getAnkiService()
    ]);
    // 关闭 AI 优选功能时：中止进行中的请求并重置卡片，界面恢复原样
    watch(() => config.llmEnabled, enabled => {
        if (!enabled) {
            aiAbort?.abort();
            aiAbort = null;
            resetAiPick();
        }
    });
    // 恢复用户选项
    const cachedDict = preference.get('selectedDict') as 'collins' | 'oxford' | 'youdao';
    if (cachedDict != null && ['collins', 'oxford', 'youdao'].includes(cachedDict)) {
        selectedDict.value = cachedDict;
    }
    const cachedPronunciation = preference.get('selectedPronunciation') as 'en' | 'us';
    if (cachedPronunciation != null && (['en', 'us'] as const).includes(cachedPronunciation)) {
        selectedPronunciation.value = cachedPronunciation;
    }
    pageInitialized.value = true;
    if (!await utils.rustInRelease()) {
        replaceSentence('The quick brown fox jumps over the lazy dog.'); // test sentence in dev mode
    }
    // 放在 dev 测试句子之后，捕获的句子可覆盖测试句子
    await initSentenceCapture();
});
</script>

<template>
    <div v-if="pageInitialized" class="main-window">
        <div class="header-container">
            <FluentButton class="header-button" :accent="showEdit" @click="changeEditStatus">
                {{ showEdit ? '完成' : '编辑' }}
            </FluentButton>
            <FluentButton class="header-button" @click="pasteToEdit">粘贴</FluentButton>
            <FluentInput class="header-input-text" type="text" v-model="searchText" placeholder="回车查询单词" name="search"
                @input="onUserEdit"
                autocomplete="off" @keydown.enter="submitSearch()" />
            <FluentButton class="header-button" @click="submitSearch()">查询
            </FluentButton>
            <FluentSelect class="header-select" v-model="selectedDict" name="dict">
                <option value="collins">柯林斯词典</option>
                <option value="oxford">新牛津英汉双解</option>
                <option value="youdao">有道在线词典</option>
            </FluentSelect>
            <SettingButton @click="router.push('/settings')" />
        </div>
        <div class="sentence-container">
            <!-- 粘贴失败的就地提示：紧挨着上方的“粘贴”按钮，收起时不占高度 -->
            <FluentStatusText class="sentence-notice" :severity="pasteNotice?.severity ?? 'error'"
                :message="pasteNotice?.message ?? null" />
            <SentencePanel class="sentence-main" :tokens="tokens" v-if="!showEdit" @mark="onMarkToken"
                @commit="onMarksCommitted" />
            <FluentTextarea class="sentence-editor sentence-main" v-model="sentence" v-if="showEdit" ref="editTextArea"
                :placeholder="editPlaceholder" @input="onSentenceEdit" @keydown="handleEditTextAreaKeydown" />
            <SentenceCaptureRing :active="captureSettling" v-if="!showEdit" />
        </div>
        <div class="words-container">
            <div class="pronunciation-container" v-show="searchText.length > 0">
                <PlayAudioButton :word="searchText" :type="selectedPronunciation"
                    @error="handlePronunciationError" />
                <FluentRadio v-model="selectedPronunciation" value="en" label="英式" name="pronunciation"
                    class="pronunciation-radio-box" />
                <FluentRadio v-model="selectedPronunciation" value="us" label="美式" name="pronunciation"
                    class="pronunciation-radio-box" />
                <!-- 发音失败就地提示：追加在行末，只做淡入淡出，不挤动左边的按钮与单选项 -->
                <FluentStatusText class="pronunciation-notice" :collapse="false"
                    :severity="pronunciationNotice?.severity ?? 'error'"
                    :message="pronunciationNotice?.message ?? null" />
            </div>
            <!-- 手打查询失败的就地提示：位于结果列表上方，收起时不占高度 -->
            <FluentStatusText class="search-notice" :severity="searchNotice?.severity ?? 'error'"
                :message="searchNotice?.message ?? null" />
            <ScrollMemory :show="selectedDict === 'collins'" class="words-container-inner">
                <!-- AI 优选卡随列表内容滚动，每个词典容器内各放一份（内容同源）；active 标记当前可见实例，仅它播放动画 -->
                <AiPickCard v-if="showAiPickCard" :state="aiPick" :status="aiPickCardStatus"
                    :picked-item="aiPickItem" :notice="aiPickNotice" :active="selectedDict === 'collins'"
                    @add-btn-click="handleAiPickAdd" @edit-btn-click="handleAiPickEdit" @stop="stopAiPick"
                    @regenerate="regenerateAiPick" @notice-action="handleAiPickNoticeAction" />
                <CollinsCard v-for="(item, index) in wordItems['collins']" :key="index" :item="item.item" :index="index"
                    :status="item.status" :notice="item.notice" :ai-picked="isAiPicked('collins', index)"
                    @add-btn-click="changeItemAdded" @edit-btn-click="openEditDialog"
                    @notice-action="handleCardNoticeAction('collins', $event)" />
            </ScrollMemory>
            <ScrollMemory :show="selectedDict === 'oxford'" class="words-container-inner">
                <AiPickCard v-if="showAiPickCard" :state="aiPick" :status="aiPickCardStatus"
                    :picked-item="aiPickItem" :notice="aiPickNotice" :active="selectedDict === 'oxford'"
                    @add-btn-click="handleAiPickAdd" @edit-btn-click="handleAiPickEdit" @stop="stopAiPick"
                    @regenerate="regenerateAiPick" @notice-action="handleAiPickNoticeAction" />
                <OxfordCard v-for="(item, index) in wordItems['oxford']" :key="index" :item="item.item" :index="index"
                    :status="item.status" :notice="item.notice" :ai-picked="isAiPicked('oxford', index)"
                    @add-btn-click="changeItemAdded" @edit-btn-click="openEditDialog"
                    @notice-action="handleCardNoticeAction('oxford', $event)" />
            </ScrollMemory>
            <ScrollMemory :show="selectedDict === 'youdao'" class="words-container-inner">
                <AiPickCard v-if="showAiPickCard" :state="aiPick" :status="aiPickCardStatus"
                    :picked-item="aiPickItem" :notice="aiPickNotice" :active="selectedDict === 'youdao'"
                    @add-btn-click="handleAiPickAdd" @edit-btn-click="handleAiPickEdit" @stop="stopAiPick"
                    @regenerate="regenerateAiPick" @notice-action="handleAiPickNoticeAction" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['concise'].length > 0" class="youdao-title">
                    简明释义
                </div>
                <YoudaoCard v-for="entry in wordItemsYoudao['concise']" :key="entry.flatIndex" :item="entry.model.item"
                    :index="entry.flatIndex" :status="entry.model.status" :notice="entry.model.notice"
                    :ai-picked="isAiPicked('youdao', entry.flatIndex)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" @notice-action="handleCardNoticeAction('youdao', $event)" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['web'].length > 0" class="youdao-title">
                    网络释义
                </div>
                <YoudaoCard v-for="entry in wordItemsYoudao['web']" :key="entry.flatIndex" :item="entry.model.item"
                    :index="entry.flatIndex" :status="entry.model.status" :notice="entry.model.notice"
                    :ai-picked="isAiPicked('youdao', entry.flatIndex)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" @notice-action="handleCardNoticeAction('youdao', $event)" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['phrase'].length > 0" class="youdao-title">
                    短语
                </div>
                <YoudaoCard v-for="entry in wordItemsYoudao['phrase']" :key="entry.flatIndex" :item="entry.model.item"
                    :index="entry.flatIndex" :status="entry.model.status" :notice="entry.model.notice"
                    :ai-picked="isAiPicked('youdao', entry.flatIndex)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" @notice-action="handleCardNoticeAction('youdao', $event)" />
            </ScrollMemory>
        </div>
    </div>
    <div v-else class="loading-screen"></div>
</template>

<style scoped>
.loading-screen {
    height: 100vh;
    background-color: var(--window-background);
}

.main-window {
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: calc(34px + 12px * 2) 1fr;
    background-color: var(--window-background);
    height: 100vh;
    /* macOS 覆盖式标题栏：顶部让出原生标题栏高度，内容不会被红绿灯压住 */
    padding-top: var(--titlebar-inset);
    grid-template-areas:
        'header header'
        'sentence words';
    overflow-x: hidden;
    user-select: none;
}

.header-container {
    grid-area: header;
    display: flex;
    justify-content: center;
    align-items: center;
    padding: 12px 15px;
    gap: 5px;
}

/* 发音行末的就地提示：与单选项留出间距；横排内不参与高度联动，也不需要顶部间距 */
.pronunciation-notice {
    --status-gap: 0px;
    margin-left: 10px;
}

.sentence-container {
    grid-area: sentence;
    padding: 15px;
    padding-top: 0;
    padding-right: calc(15px / 2);
    /* 需要设置此属性才能让 SentencePanel 的 overflow-y: auto 生效 */
    overflow-y: hidden;
    /* SentenceCaptureRing 以此为定位基准，钉在句子面板右下角 */
    position: relative;
    /* 纵向排布：顶部可能有一行粘贴失败提示，句子面板（或编辑框）占满其余高度 */
    display: flex;
    flex-direction: column;
}

/* 句子面板与编辑框：占满提示行之外的高度（flex-basis 0 让自身的 height: 100% 不再生效） */
.sentence-main {
    flex: 1 1 0;
    min-height: 0;
}

/* 句子区与结果区上方的就地提示：间距放在提示下方，并留在被折叠的区域内，收起时不留空档 */
.sentence-notice,
.search-notice {
    --status-gap: 0px;
    flex-shrink: 0;
}

.sentence-notice :deep(.status-text),
.search-notice :deep(.status-text) {
    padding-bottom: 8px;
}

/* 与结果列表左右内边距对齐 */
.search-notice {
    padding-left: calc(15px / 2);
    padding-right: 15px;
}

.words-container {
    grid-area: words;
    overflow-y: hidden;
    display: flex;
    flex-direction: column;
}

.words-container-inner {
    flex-grow: 1;
    overflow-y: auto;
    padding: 15px;
    padding-top: 0;
    padding-left: calc(15px / 2);
}

.pronunciation-container {
    padding-top: 0;
    padding-left: calc(15px / 2);
    padding-bottom: 8px;
    display: flex;
    align-items: center;
}

.pronunciation-radio-box {
    margin-right: 8px;
}

.header-button {
    height: 34px;
    padding-left: 12px;
    padding-right: 12px;
}

.header-select {
    height: 34px;
}

.header-input-text {
    height: 34px;
    flex-grow: 1;
}

.sentence-editor {
    width: 100%;
    height: 100%;
}

.youdao-title {
    margin-bottom: 10px;
    user-select: none;
    cursor: default;
}
</style>
