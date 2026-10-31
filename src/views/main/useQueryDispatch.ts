import { watch } from 'vue';

import type { AiPick } from './useAiPick';
import type { WordSearch } from './useWordSearch';

export interface UseQueryDispatchOptions {
    search: Pick<WordSearch, 'searchText' | 'selectedDict' | 'searchAndUpdate' | 'searchAsTyped' | 'dismissSearchNotice'>;
    ai: Pick<AiPick, 'debouncedAiPick' | 'maybeStartAiPick' | 'retriggerIfCandidatesGrew'>;
}

/**
 * 查询的调度：搜索框或所选词典变化时决定查哪本词典、要不要连 AI 一起查。
 * 手打走“节流词典 + 防抖 AI”，显式写入（标记快送、划词捕获、回车/查询按钮）立即连 AI 一起查
 */
export function useQueryDispatch(options: UseQueryDispatchOptions) {
    const { search, ai } = options;
    const { searchText, selectedDict } = search;

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

    /** 提交一次有效搜索：更新当前词典的搜索结果并立即触发 AI 优选（回车、查询按钮等显式动作） */
    async function submitSearch() {
        await search.searchAndUpdate(searchText.value, selectedDict.value);
        void ai.maybeStartAiPick(searchText.value);
    }

    /** 所选的词典改变时，在所选词典中搜索新单词 */
    watch(selectedDict, async newSelected => {
        // 上一本词典的手打查询失败提示不属于新词典的结果列表
        search.dismissSearchNotice();
        const succeeded = await search.searchAndUpdate(searchText.value, newSelected);
        if (succeeded) {
            ai.retriggerIfCandidatesGrew();
        }
    });

    /** 搜索框中的文字改变时：手打走“节流词典 + 防抖 AI”，显式写入立即连 AI 一起查 */
    watch(searchText, () => {
        if (queryOrigin === 'explicit') {
            queryOrigin = 'typing';
            // 显式动作优先：取消手打期间待发的 AI 防抖，避免它在新查询发起途中插队
            ai.debouncedAiPick.cancel();
            void submitSearch();
            return;
        }
        search.searchAsTyped();
        ai.debouncedAiPick(searchText.value);
    });

    return { setQueryExplicitly, submitSearch };
}
