import { beforeEach, describe, expect, it, vi } from 'vitest';
import { effectScope } from 'vue';

import * as dict from '../../logics/dict';
import { useWordSearch } from './useWordSearch';

vi.mock('../../logics/dict', () => ({
    searchCollins: vi.fn(),
    searchOxford: vi.fn(),
    searchYoudao: vi.fn(),
}));
vi.mock('../../logics/preference', () => ({
    get: vi.fn(() => null),
    set: vi.fn(),
}));
vi.mock('../../fluent-controls', () => ({
    dialog: { message: vi.fn(() => Promise.resolve()) },
}));

function youdaoItem(word: string): dict.YoudaoItem {
    return { word, phonetic: null, sense: 'n.', cnDef: '狐狸', meaningType: 'concise' };
}

function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (reason?: unknown) => void;
    const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
    return { promise, resolve, reject };
}

beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(console, 'error').mockImplementation(() => { });
});

/** 在独立的 effect scope 里创建，避免 useTransientNotice 的 onScopeDispose 报警告 */
function setup() {
    return effectScope().run(() => useWordSearch())!;
}

describe('searchAndUpdate：同一个词的并发请求', () => {
    it('后来的调用等待在途请求，不会在结果回来之前返回', async () => {
        const pending = deferred<dict.YoudaoItem[]>();
        vi.mocked(dict.searchYoudao).mockReturnValue(pending.promise);

        const search = setup();
        const first = search.searchAndUpdate('fox', 'youdao');
        const second = search.searchAndUpdate('fox', 'youdao');

        // 关键：第二个调用不能先返回——它返回 true 时调用方会立刻读 wordItems
        let secondSettled = false;
        void second.then(() => { secondSettled = true; });
        await Promise.resolve();
        expect(secondSettled).toBe(false);
        expect(dict.searchYoudao).toHaveBeenCalledTimes(1);

        pending.resolve([youdaoItem('fox')]);
        await expect(first).resolves.toBe(true);
        await expect(second).resolves.toBe(true);
        expect(search.wordItems.youdao).toHaveLength(1);
    });

    it('已搜到的词直接返回，不再发请求', async () => {
        vi.mocked(dict.searchYoudao).mockResolvedValue([youdaoItem('fox')]);

        const search = setup();
        await expect(search.searchAndUpdate('fox', 'youdao')).resolves.toBe(true);
        await expect(search.searchAndUpdate('fox', 'youdao')).resolves.toBe(true);
        expect(dict.searchYoudao).toHaveBeenCalledTimes(1);
    });

    it('查询失败在列表上方就地提示，并点名是哪本词典', async () => {
        vi.mocked(dict.searchYoudao).mockRejectedValue(new Error('HTTP 500'));

        const search = setup();
        await expect(search.searchAndUpdate('fox', 'youdao')).resolves.toBe(false);
        expect(search.searchNotice.value?.message).toContain('有道词典查询失败');
        expect(search.searchNotice.value?.message).toContain('HTTP 500');
    });

    it('在途请求失败时，后来的调用拿到失败而不是“已查过”', async () => {
        const pending = deferred<dict.YoudaoItem[]>();
        vi.mocked(dict.searchYoudao).mockReturnValue(pending.promise);

        const search = setup();
        const first = search.searchAndUpdate('fox', 'youdao');
        const second = search.searchAndUpdate('fox', 'youdao');

        pending.reject(new Error('boom'));
        await expect(first).resolves.toBe(false);
        await expect(second).resolves.toBe(false);
    });
});
