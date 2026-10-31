import { computed, reactive, ref, watch } from 'vue';

import * as utils from '../../logics/utils';
import * as dict from '../../logics/dict';
import * as preference from '../../logics/preference';
import { dialog } from '../../fluent-controls';
import { useTransientNotice } from '../../fluent-controls/useTransientNotice';
import type { DictName, ItemModel, WordItems } from './types';

/** 单词搜索：搜索框、词典与发音选项、各词典的搜索结果 */
export function useWordSearch() {
    /**
     * 搜索框文本：查询的唯一真源。只由“用户授权的输入”写入——用户手打，或用户标记手势结束时
     * 把标记词元快送进来（见 MainView 的 onMarksCommitted）；捕获初始化属初始状态的一部分，也直接写入
     * （见 useCaptureListener 的 applyCapturedSentence）。
     *
     * 反向不成立：标记状态不是 searchText 的函数变换。标记只是展示状态与“快送”手势的产物，
     * 程序化写 marked（取句预选、编辑重建分词）绝不改写查询——分词粒度与选区不一致时
     * （如只选中 encoder-decoder 里的 encoder），那个词元上的标记与查询本就允许不等价。
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
     * @param options.errorFeedback 查询失败时的反馈方式：dialog（默认，显式查询）弹错误框；
     *   inline 在结果列表上方就地提示（手打查询：逐键弹模态框会打断输入）；
     *   silent 只 console.error（AI 预取静默重试）
     * @returns 查询失败时返回 false，其余路径（含去重早退、空词清空、成功）返回 true
     */
    async function searchAndUpdate(
        word: string,
        dictionary: DictName,
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
