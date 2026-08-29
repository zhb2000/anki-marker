<script setup lang="ts">
import { ref, reactive, watch, nextTick, onBeforeMount, computed } from 'vue';
import { useRouter } from 'vue-router';
import * as api from '../tauri-api';
import { ElMessage, type MessageHandler } from 'element-plus';

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
import { FluentButton, FluentSelect, FluentInput, FluentRadio } from '../fluent-controls';
import {
    CardStatus,
    SentencePanel,
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
}

/** 划词捕获结果：text 为录入文本（取句成功时是整句），word 为取句模式命中的单词（用于预选，可能为 null） */
interface CapturedSentencePayload {
    text: string;
    word: string | null;
}

/** 划词面板的词元 */
const tokens = ref<{ token: string, marked: boolean; }[]>([]);
/** 所选的字典 */
const selectedDict = ref<'collins' | 'oxford' | 'youdao'>('collins');
/** 搜索框文本 */
const searchText = ref('');
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

/** 拖刷手势进行中：词元标记在手势内连续变化，抑制逐词搜索，手势结束后统一提交一次 */
const painting = ref(false);

/** 按当前标记状态同步 searchText 并提交搜索 */
function syncSearchFromTokens() {
    const newSearchText = tokens.value
        .filter(token => token.marked)
        .map(token => token.token)
        .join(' ');
    searchText.value = newSearchText;
    return submitSearch();
}

/** tokens 的选中状态改变时，更新 searchText 并搜索新单词；拖刷手势进行中则等手势结束再统一搜索 */
watch(tokens, () => {
    if (!painting.value) {
        void syncSearchFromTokens();
    }
}, { deep: true });

function onPaintStart(): void {
    painting.value = true;
}

function onPaintEnd(): void {
    painting.value = false;
    void syncSearchFromTokens();
}

/** SentencePanel 上报的标记写入：tokens 的写入收敛于此（整体替换与编辑重建除外），搜索仍由上方 deep watch 统一触发 */
function onMarkToken(index: number, value: boolean): void {
    tokens.value[index].marked = value;
}

/** 所选的词典改变时，在所选词典中搜索新单词 */
watch(selectedDict, async newSelected => {
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

/** 搜索框中的文字改变时，搜索新单词 */
watch(searchText, () => {
    if (selectedDict.value === 'youdao') {
        throttledYoudaoSearch();
    } else {
        throttledSearch();
    }
});

/** 保存所选的字典 */
watch(selectedDict, newDict => {
    preference.set('selectedDict', newDict);
});

/** 保存所选的发音 */
watch(selectedPronunciation, newPronunciation => {
    preference.set('selectedPronunciation', newPronunciation);
});

/** 输入框内容改变时调用节流版搜索函数，避免过于频繁地搜索 */
const throttledSearch = utils.throttle(submitSearch, 200);

const throttledYoudaoSearch = utils.throttle(submitSearch, 400);

/**
 * 搜索单词，并用搜索结果更新 wordItems。
 *
 * 点击“查询”按钮或按下回车键时直接调用此函数。
 *
 * @param options.suppressErrorDialog 为 true 时查询失败只 console.error 不弹 dialog（供 AI 预取静默重试）
 * @returns 查询失败时返回 false，其余路径（含去重早退、空词清空、成功）返回 true
 */
async function searchAndUpdate(
    word: string,
    dictionary: 'collins' | 'oxford' | 'youdao',
    options?: { suppressErrorDialog?: boolean; }
): Promise<boolean> {
    word = word.trim();
    if (word.length === 0) {
        wordItems[dictionary] = [];
        // 界面上的单词列表已清空，无法复用，因此对于任何单词都要重新搜索
        searchingOrSearchedWords[dictionary] = '';
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
            if (options?.suppressErrorDialog !== true) {
                await api.dialog.message(String(error), { title: '查询失败', kind: 'error' });
            }
        }
        return false;
    }
    // 仅在本次搜索结果有效时更新搜索结果单词列表
    if (searchingOrSearchedWords[dictionary] === word) {
        wordItems[dictionary] = results.map(item => ({ item, status: 'not-added', id: null }) as ItemModel<any>);
    }
    return true;
}
// #endregion

// #region 文本框句子编辑
/** 文本框中的句子 */
const sentence = ref('');
/** 是否显示文本框 */
const showEdit = ref(false);
/** 文本框控件 */
const editTextArea = ref<HTMLTextAreaElement | null>(null);
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
 * 程序化整体替换句子（划词录入、粘贴、dev 测试句等）：
 * 句子与分词同步重建为全新状态（清除已有标记），并作废进行中的编辑草稿。
 * 值相同也重建——每次整体替换都应是全新状态。
 */
function replaceSentence(text: string): void {
    editBaselineTokens = null;
    if (sentence.value !== text) {
        sentence.value = text;
    }
    tokens.value = utils.string.tokenize(text).map(token => ({ token, marked: false }));
}

async function pasteToEdit() {
    const text = await api.clipboard.readText();
    if (text != null) {
        // 粘贴属于整体替换句子：分词同步重建为全新状态，不保留旧的标记，编辑草稿一并作废
        replaceSentence(text.trim());
        if (showEdit.value) {
            await changeEditStatus();
        }
    }
}

async function changeEditStatus() {
    showEdit.value = !showEdit.value;
    if (showEdit.value) {
        // 进入编辑模式：快照当前分词作为基线，编辑期间不再重建分词
        editBaselineTokens = tokens.value.map(({ token, marked }) => ({ token, marked }));
        await nextTick();
        editTextArea.value?.focus();
    } else {
        // 确认编辑（完成）：以基线分词为基准重建，尽量保留已标记单词。
        // 基线为 null 说明编辑期间句子被整体替换（划词录入、粘贴），分词已是全新状态，无需重建
        if (editBaselineTokens != null) {
            tokens.value = utils.string.rebuildTokensPreservingMarks(editBaselineTokens, sentence.value);
            editBaselineTokens = null;
        }
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

/** AI 优选命中的词典条目（供 AiPickCard 渲染完整内容）；pick 为空或条目缺失时为 null */
const aiPickItem = computed(() => {
    if (aiPick.pick == null) return null;
    const item = wordItems[aiPick.pick.source][aiPick.pick.index]?.item;
    return item != null ? { source: aiPick.pick.source, item } : null;
});

/**
 * 提交一次有效搜索后调用：若启用并配置好了 LLM，则并行搜索三本词典（填满 wordItems，
 * 保证候选与界面条目索引对齐、切 tab 即时）并发起 AI 优选；否则在无进行中请求时重置为 idle。
 */
async function maybeStartAiPick(word: string) {
    const trimmed = word.trim();
    const llmConfig: LlmRequestConfig = {
        baseUrl: config.llmBaseUrl,
        apiKey: config.llmApiKey,
        model: config.llmModel,
        maxTokens: parseMaxTokens(config.llmMaxTokens),
        reasoningEffort: config.llmReasoningEffort
    };
    if (trimmed.length === 0 || !config.llmEnabled || !isLlmReady(llmConfig)) {
        if (aiAbort == null) {
            resetAiPick();
        }
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
            let ok = await searchAndUpdate(trimmed, dictionary, { suppressErrorDialog: true });
            if (!ok) {
                console.warn(`[aiPick] ${dictionary} 预取失败，重试一次`);
                ok = await searchAndUpdate(trimmed, dictionary, { suppressErrorDialog: true });
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

/** 提交一次有效搜索：更新当前词典的搜索结果，并按需触发 AI 优选 */
async function submitSearch() {
    await searchAndUpdate(searchText.value, selectedDict.value);
    void maybeStartAiPick(searchText.value);
}

/** sentence 或 searchText 变化时，若无进行中的 AI 请求，则把 AI 卡片重置为 idle */
watch([sentence, searchText], () => {
    if (aiAbort == null && aiPick.phase !== 'idle') {
        resetAiPick();
    }
});
// #endregion

// #region 全局快捷键划词录入
/** 录入捕获的句子：整体替换当前句子（分词同步重建为全新状态）并退出编辑模式；取句模式命中的单词会在分词结果中预选 */
async function applyCapturedSentence(payload: CapturedSentencePayload) {
    const trimmed = payload.text.trim();
    if (trimmed.length === 0) {
        return;
    }
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
        // replaceSentence 已同步重建分词，可直接标记命中的单词
        markCapturedWord(word);
    }
}

/**
 * 在分词结果中标记捕获的单词：先精确匹配单词元，再忽略大小写，最后按连续词元序列匹配短语；都不匹配则不预选
 */
function markCapturedWord(word: string): void {
    const list = tokens.value;
    // 单词匹配：精确，失败再忽略大小写
    let index = list.findIndex(t => t.token === word);
    if (index < 0) {
        const lower = word.toLowerCase();
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
}

/** 监听划词句子事件，并取走可能在主窗口重建期间暂存的句子 */
async function initSentenceCapture() {
    // 主窗口被关闭后按快捷键会重建窗口，前端就绪之前 emit 的事件会丢失，
    // 此时通过 take_pending_sentence 取回 Rust 侧暂存的句子
    try {
        await api.event.listen<CapturedSentencePayload>('sentence-captured', event => {
            void applyCapturedSentence(event.payload);
        });
        const pending = await utils.invoke<CapturedSentencePayload | null>('take_pending_sentence');
        if (pending != null) {
            await applyCapturedSentence(pending);
        }
    } catch (error) {
        console.error(error);
    }
    try {
        await api.event.listen('sentence-capture-failed', () => {
            void api.dialog.message(
                '获取选中文本失败。\n\n请在“系统设置 → 隐私与安全性 → 辅助功能”中允许本应用，然后重试。',
                { title: '划词录入失败', kind: 'error' }
            );
        });
    } catch (error) {
        console.error(error);
    }
}
// #endregion

// #region Anki
/** 若牌组或笔记模板不存在，则创建之 */
async function prepareDeckAndModel(deckName: string, modelName: string) {
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
        await api.dialog.message(String(error), { title: errorTitle!, kind: 'error' });
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
 * 切换指定词典中指定条目的加卡状态（添加/取消添加）。
 *
 * 词典参数独立出来，是为了让 AI 优选卡片能跨词典操作其选中的条目（可能与当前 tab 不同）；
 * AI 笔记注入判断仍用 isAiPicked(dictionary, index)，仅命中条目带笔记时写入“笔记”字段
 */
async function changeItemAddedOf(dictionary: 'collins' | 'oxford' | 'youdao', index: number) {
    const item = wordItems[dictionary][index];
    const pronunciationType = selectedPronunciation.value;
    if (item.status === 'not-added') { // add to Anki
        item.status = 'processing-add';
        // 确保 AnkiConnect 可用：不可用时按配置自动启动 Anki 并等待其就绪
        let progressMessage: MessageHandler | null = null;
        // 关闭进度提示。通过箭头函数引用 progressMessage，
        // 避免 TS 沿用外层将其窄化为 null 而导致 finally 中调用 close() 报类型错误
        const closeProgressMessage = () => { progressMessage?.close(); };
        try {
            await globals.ensureAnkiConnect(message => {
                closeProgressMessage();
                progressMessage = ElMessage({ message, duration: 0, showClose: true });
            });
        } catch (error) {
            item.status = 'not-added';
            console.error(error);
            await api.dialog.message(String(error), { title: '无法连接 Anki', kind: 'error' });
            return;
        } finally {
            closeProgressMessage();
        }
        try {
            await prepareDeckAndModel(config.deckName, config.modelName);
        } catch (error) {
            item.status = 'not-added';
            console.error(error);
            return; // prepareDeckAndModel has already shown the error message
        }
        try {
            const fields = anki.makeFields(dictionary, item.item, makeSentenceHTML());
            // AI 优选联动：若所加条目正是 AI 优选命中的条目且带有笔记，则写入“笔记”字段
            if (aiPick.note.length > 0 && isAiPicked(dictionary, index)) {
                fields['笔记'] = aiPick.note;
            }
            const word = ('phrase' in item.item && item.item.phrase != null)
                ? item.item.phrase
                : item.item.word;
            let audioResult = await dict.makePronunciationURL(word, pronunciationType);
            if (audioResult == null) {
                audioResult = {
                    url: dict.makeYoudaoDictVoiceUrl(word, pronunciationType),
                    dict: 'youdao'
                };
            }
            const audioFilename = await dict.makePronunciationFilename(word, pronunciationType, audioResult.dict);
            const id = await ankiService.addMarkerNote(
                config.deckName,
                config.modelName,
                fields,
                audioResult.url,
                audioFilename
            );
            if (id == null) {
                throw new Error('addNote returns null');
            }
            item.id = id;
            item.status = 'is-added';
        } catch (error) {
            item.status = 'not-added';
            console.error(error);
            await api.dialog.message(String(error), { title: '添加失败', kind: 'error' });
        }
    } else if (item.status === 'is-added') { // remove from Anki
        item.status = 'processing-remove';
        try {
            await ankiService.deleteNotes([item.id!]);
            item.id = null;
            item.status = 'not-added';
        } catch (error) {
            item.status = 'is-added';
            console.error(error);
            await api.dialog.message(String(error), { title: '删除失败', kind: 'error' });
        }
    }
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
async function openEditDialog(index: number) {
    const selected = selectedDict.value;
    const item = wordItems[selected][index];
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
            ElMessage.warning({
                message: '编辑对话框打开失败（Anki 26.08 与 AnkiConnect 不兼容），已在 Anki 卡片浏览器中定位该笔记，选中后可在右侧直接编辑。',
                duration: 5000
            });
        } catch (fallbackError) {
            console.error(fallbackError);
            await api.dialog.message(String(error), { title: '打开编辑对话框失败', kind: 'error' });
        }
    }
}

function makeSentenceHTML(): string {
    return tokens.value.map(
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
                autocomplete="off" @keydown.enter="submitSearch()" />
            <FluentButton class="header-button" @click="submitSearch()">查询
            </FluentButton>
            <FluentSelect class="header-select" v-model="selectedDict" name="dict">
                <option value="collins">柯林斯词典</option>
                <option value="oxford">新牛津英汉双解</option>
                <option value="youdao">有道在线词典</option>
            </FluentSelect>
            <SettingButton @click="router.push('/settings')"
                :update-available="globals.appUpdateAvailable.value || globals.templateUpdateAvailable.value" />
        </div>
        <div class="sentence-container">
            <SentencePanel :tokens="tokens" v-if="!showEdit" @mark="onMarkToken" @paint-start="onPaintStart" @paint-end="onPaintEnd" />
            <textarea class="fluent-textarea" v-model.trim="sentence" v-if="showEdit" ref="editTextArea"
                :placeholder="editPlaceholder" @keydown="handleEditTextAreaKeydown"></textarea>
        </div>
        <div class="words-container">
            <div class="pronunciation-container" v-show="searchText.length > 0">
                <PlayAudioButton :word="searchText" :type="selectedPronunciation" />
                <FluentRadio v-model="selectedPronunciation" value="en" label="英式" name="pronunciation"
                    class="pronunciation-radio-box" />
                <FluentRadio v-model="selectedPronunciation" value="us" label="美式" name="pronunciation"
                    class="pronunciation-radio-box" />
            </div>
            <ScrollMemory :show="selectedDict === 'collins'" class="words-container-inner">
                <!-- AI 优选卡随列表内容滚动，每个词典容器内各放一份（内容同源）；active 标记当前可见实例，仅它播放动画 -->
                <AiPickCard v-if="showAiPickCard" :state="aiPick" :status="aiPickCardStatus"
                    :picked-item="aiPickItem" :active="selectedDict === 'collins'" @add-btn-click="handleAiPickAdd" />
                <CollinsCard v-for="(item, index) in wordItems['collins']" :key="index" :item="item.item" :index="index"
                    :status="item.status" :ai-picked="isAiPicked('collins', index)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" />
            </ScrollMemory>
            <ScrollMemory :show="selectedDict === 'oxford'" class="words-container-inner">
                <AiPickCard v-if="showAiPickCard" :state="aiPick" :status="aiPickCardStatus"
                    :picked-item="aiPickItem" :active="selectedDict === 'oxford'" @add-btn-click="handleAiPickAdd" />
                <OxfordCard v-for="(item, index) in wordItems['oxford']" :key="index" :item="item.item" :index="index"
                    :status="item.status" :ai-picked="isAiPicked('oxford', index)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" />
            </ScrollMemory>
            <ScrollMemory :show="selectedDict === 'youdao'" class="words-container-inner">
                <AiPickCard v-if="showAiPickCard" :state="aiPick" :status="aiPickCardStatus"
                    :picked-item="aiPickItem" :active="selectedDict === 'youdao'" @add-btn-click="handleAiPickAdd" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['concise'].length > 0" class="youdao-title">
                    简明释义
                </div>
                <YoudaoCard v-for="entry in wordItemsYoudao['concise']" :key="entry.flatIndex" :item="entry.model.item"
                    :index="entry.flatIndex" :status="entry.model.status"
                    :ai-picked="isAiPicked('youdao', entry.flatIndex)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['web'].length > 0" class="youdao-title">
                    网络释义
                </div>
                <YoudaoCard v-for="entry in wordItemsYoudao['web']" :key="entry.flatIndex" :item="entry.model.item"
                    :index="entry.flatIndex" :status="entry.model.status"
                    :ai-picked="isAiPicked('youdao', entry.flatIndex)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['phrase'].length > 0" class="youdao-title">
                    短语
                </div>
                <YoudaoCard v-for="entry in wordItemsYoudao['phrase']" :key="entry.flatIndex" :item="entry.model.item"
                    :index="entry.flatIndex" :status="entry.model.status"
                    :ai-picked="isAiPicked('youdao', entry.flatIndex)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" />
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

.sentence-container {
    grid-area: sentence;
    padding: 15px;
    padding-top: 0;
    padding-right: calc(15px / 2);
    /* 需要设置此属性才能让 SentencePanel 的 overflow-y: auto 生效 */
    overflow-y: hidden;
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

.fluent-textarea {
    resize: none;
    overflow-wrap: break-word;
    width: 100%;
    height: 100%;
    padding: 8px 12px;
    font-family: var(--font-family);
    font-size: var(--font-size);
    background-color: var(--control-background);
    outline: none;
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
}

.fluent-textarea:hover {
    background-color: var(--control-background-hover);
}

.fluent-textarea:focus {
    background-color: var(--input-text-background-focus);
    border-bottom-width: var(--input-text-border-bottom-width-focus);
    border-bottom-color: var(--accent);
}

.youdao-title {
    margin-bottom: 10px;
    user-select: none;
    cursor: default;
}
</style>
