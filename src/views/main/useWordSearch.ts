import { computed, reactive, ref, watch } from 'vue';

import * as utils from '../../logics/utils';
import * as dict from '../../logics/dict';
import * as preference from '../../logics/preference';
import { useTransientNotice } from '../../fluent-controls/useTransientNotice';
import { DICT_LABELS, type DictName, type ItemModel, type WordItems } from './types';

/** 单词搜索：搜索框、词典与发音选项、各词典的搜索结果 */
export function useWordSearch() {
    /**
     * 搜索框文本：查询的唯一真源。只由“用户授权的输入”写入——用户手打，或选区手势结束时
     * 把选中的词快送进来（见 MainView 的 onSelectionCommitted）；捕获初始化属初始状态的一部分，也直接写入
     * （见 useCaptureListener 的 applyCapturedSentence）。
     *
     * 反向不成立：选区不是 searchText 的函数变换。选区只是展示状态与“快送”手势的产物，
     * 程序化写选区（取句预选、编辑重建分词）绝不改写查询——分词粒度与选区不一致时
     * （如只选中 encoder-decoder 里的 encoder），那个词元上的选区与查询本就允许不等价。
     */
    const searchText = ref('');
    /** 所选的字典 */
    const selectedDict = ref<DictName>('collins');
    /** 所选的发音 */
    const selectedPronunciation = ref<'en' | 'us'>('us');

    /** 每本字典中的搜索结果 */
    const wordItems = reactive<WordItems>({
        'collins': [],
        'oxford': [],
        'youdao': []
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
    /**
     * 进行中的请求。同一个词典同一个词被重复请求时，后来的调用等这一次结束再返回——
     * 界面刷新不关心这个差别，但 AI 预取拿到返回值后会立刻读 wordItems，
     * 若这里直接按“已查过”返回，它读到的是还没填上的空列表，会把“还没回来”当成“词典没收录”
     */
    const searchJobs: Record<DictName, { word: string; promise: Promise<boolean>; } | null> = {
        'collins': null,
        'oxford': null,
        'youdao': null
    };

    /** 发音失败的就地提示：挂在发音行末尾，不挤动按钮与单选项 */
    const { notice: pronunciationNotice, show: showPronunciationNotice } = useTransientNotice(3000);

    /** 发音按钮报告失败：就地显示 3s */
    function handlePronunciationError(message: string): void {
        showPronunciationNotice(message, { severity: 'error' });
    }

    /**
     * 手打查询失败的就地提示（显示在结果列表上方）。常驻到该词典下一次查询成功、
     * 查询词被清空或切换词典为止：网络持续故障时边打字边失败，提示保持展开只更新文案，不反复伸缩
     */
    const { notice: searchNotice, show: showSearchNotice, clear: clearSearchNotice } = useTransientNotice(0);
    /** 当前就地提示属于哪本词典：别的词典查询成功不应收起它 */
    let searchNoticeDict: DictName | null = null;

    function clearSearchNoticeOf(dictionary: DictName): void {
        if (searchNoticeDict === dictionary) {
            searchNoticeDict = null;
            clearSearchNotice();
        }
    }

    /** 收起手打查询失败的提示，不论它属于哪本词典（切换词典时：上一本词典的失败提示不属于新词典的结果列表） */
    function dismissSearchNotice(): void {
        searchNoticeDict = null;
        clearSearchNotice();
    }

    /**
     * 搜索单词，并用搜索结果更新 wordItems。
     *
     * 点击“查询”按钮或按下回车键时直接调用此函数。
     *
     * @param options.errorFeedback 查询失败时的反馈方式：inline（默认）在结果列表上方就地提示；
     *   silent 只 console.error（AI 预取）。不用模态框：查询失败是“这个列表为什么是空的”的
     *   原因，贴在列表上比抢焦点弹窗更贴合语境，也不会在连续失败时排队弹框
     * @returns 查询失败时返回 false，其余路径（含去重命中、空词清空、成功）返回 true。
     *   同一个词正在请求时等待那次请求结束：返回 true 即表示 wordItems 已更新完毕
     */
    async function searchAndUpdate(
        word: string,
        dictionary: DictName,
        options?: { errorFeedback?: 'inline' | 'silent'; }
    ): Promise<boolean> {
        word = word.trim();
        if (word.length === 0) {
            wordItems[dictionary] = [];
            // 界面上的单词列表已清空，无法复用，因此对于任何单词都要重新搜索
            searchingOrSearchedWords[dictionary] = '';
            clearSearchNoticeOf(dictionary);
            return true;
        }
        const pending = searchJobs[dictionary];
        if (pending != null && pending.word === word) {
            // 同一个词正在请求：等它结束再返回，不要当成“已查过”。
            // 错误反馈沿用最初那次调用，重复调用不再弹第二次提示
            return pending.promise;
        }
        if (word === searchingOrSearchedWords[dictionary]) {
            // 上次已搜到：复用界面上的单词列表
            return true;
        }

        const job = { word, promise: runSearch(word, dictionary, options) };
        searchJobs[dictionary] = job;
        try {
            return await job.promise;
        } finally {
            // 只清理自己那一份：期间可能已被新的查询顶替
            if (searchJobs[dictionary] === job) {
                searchJobs[dictionary] = null;
            }
        }
    }

    /** searchAndUpdate 的请求部分：去重与在途等待都在调用方，这里只管查一次并更新列表 */
    async function runSearch(
        word: string,
        dictionary: DictName,
        options?: { errorFeedback?: 'inline' | 'silent'; }
    ): Promise<boolean> {
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
                if ((options?.errorFeedback ?? 'inline') === 'inline') {
                    // 点名是哪本词典：提示位于整个结果区顶部，不写来源就分不清是哪个列表失败了
                    showSearchNotice(
                        `${DICT_LABELS[dictionary]}词典查询失败：${error instanceof Error ? error.message : String(error)}`,
                        { severity: 'error', duration: 0 }
                    );
                    searchNoticeDict = dictionary;
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
     * 只查当前词典，不触发 AI 优选：手打路径专用（AI 由 debouncedAiPick 另行发起）。
     * 失败只就地提示：网络故障时每个节流后的查询都会失败，弹模态框会排队弹出并抢走输入焦点；
     * 回车或点“查询”时才弹框
     */
    async function searchDictionary(): Promise<void> {
        await searchAndUpdate(searchText.value, selectedDict.value, { errorFeedback: 'inline' });
    }

    /** 手打时只查词典（便宜、可随输入更新），节流避免过于频繁的请求 */
    const throttledSearch = utils.throttle(searchDictionary, 200);

    const throttledYoudaoSearch = utils.throttle(searchDictionary, 400);

    /** 手打查询：按当前词典节流查询（有道是在线接口，节流间隔更长） */
    function searchAsTyped(): void {
        if (selectedDict.value === 'youdao') {
            throttledYoudaoSearch();
        } else {
            throttledSearch();
        }
    }

    /** 保存所选的字典 */
    watch(selectedDict, newDict => {
        preference.set('selectedDict', newDict);
    });

    /** 保存所选的发音 */
    watch(selectedPronunciation, newPronunciation => {
        preference.set('selectedPronunciation', newPronunciation);
    });

    /** 恢复用户选项 */
    function restorePreferences(): void {
        const cachedDict = preference.get('selectedDict') as DictName;
        if (cachedDict != null && ['collins', 'oxford', 'youdao'].includes(cachedDict)) {
            selectedDict.value = cachedDict;
        }
        const cachedPronunciation = preference.get('selectedPronunciation') as 'en' | 'us';
        if (cachedPronunciation != null && (['en', 'us'] as const).includes(cachedPronunciation)) {
            selectedPronunciation.value = cachedPronunciation;
        }
    }

    return {
        searchText,
        selectedDict,
        selectedPronunciation,
        wordItems,
        wordItemsYoudao,
        pronunciationNotice,
        handlePronunciationError,
        searchNotice,
        dismissSearchNotice,
        searchAndUpdate,
        searchAsTyped,
        restorePreferences,
    };
}

export type WordSearch = ReturnType<typeof useWordSearch>;
