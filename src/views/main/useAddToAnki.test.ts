import { beforeEach, describe, expect, it, vi } from 'vitest';
import { reactive, ref } from 'vue';

import type { AiPickState } from '../../logics/aiPick';
import type { AnkiService } from '../../logics/anki';
import type { Config } from '../../logics/config';
import type { CollinsItem } from '../../logics/dict';
import { decodeHtmlEntities } from '../../logics/stringutils';
import { createCardNotices } from './cardNotices';
import { aiNoteFor, useAddToAnki } from './useAddToAnki';
import type { DictName, WordItems } from './types';

const mocks = vi.hoisted(() => ({
    ensureAnkiConnect: vi.fn<(onProgress?: (message: string) => void, options?: { signal?: AbortSignal; }) => Promise<void>>(),
    makePronunciationURL: vi.fn<(word: string, type: 'en' | 'us') => Promise<{ url: string; dict: 'youdao' | 'iciba'; } | null>>(),
    makePronunciationFilename: vi.fn<(word: string, type: 'en' | 'us', dict: 'youdao' | 'iciba') => Promise<string>>(),
    dialogMessage: vi.fn<(message: string, options: { title: string; kind: string; }) => Promise<void>>(),
}));

vi.mock('../../logics/globals', () => ({
    ensureAnkiConnect: mocks.ensureAnkiConnect,
    templateVersion: { value: '1.0.0' },
    fetchAndSetTemplateVersion: vi.fn(),
}));
vi.mock('../../logics/dict', () => ({
    makePronunciationURL: mocks.makePronunciationURL,
    makePronunciationFilename: mocks.makePronunciationFilename,
}));
vi.mock('../../fluent-controls', () => ({
    dialog: { message: mocks.dialogMessage },
}));

const fox: CollinsItem = { word: 'fox', phonetic: null, sense: 'N-COUNT', enDef: null, cnDef: '狐狸' };
const dog: CollinsItem = { word: 'dog', phonetic: null, sense: 'N-COUNT', enDef: null, cnDef: '狗' };

function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (reason?: unknown) => void;
    const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
    return { promise, resolve, reject };
}

function setup() {
    const wordItems = reactive<WordItems>({
        collins: [
            { item: fox, status: 'not-added', id: null },
            { item: dog, status: 'not-added', id: null },
        ],
        oxford: [],
        youdao: [],
    });
    const selectedDict = ref<DictName>('collins');
    const selectedPronunciation = ref<'en' | 'us'>('us');
    const aiPick = reactive<AiPickState>({
        phase: 'done',
        pick: { source: 'collins', index: 0 },
        contextualDef: '狐狸',
        note: 'sly as a <fox> & co.',
        fallback: false,
    });
    const page = { sentenceHTML: 'The quick <b>fox</b>.' };
    const config = { deckName: '  ', modelName: 'Marker Model' } as Config;
    const ankiService = {
        createDeck: vi.fn<(deck: string) => Promise<number>>(() => Promise.resolve(1)),
        modelNames: vi.fn<() => Promise<string[]>>(() => Promise.resolve(['Marker Model'])),
        createMarkerModel: vi.fn<(modelName: string) => Promise<void>>(() => Promise.resolve()),
        addMarkerNote: vi.fn<AnkiService['addMarkerNote']>(() => Promise.resolve(1001)),
        findAddedMarkerNote: vi.fn<AnkiService['findAddedMarkerNote']>(() => Promise.resolve(null)),
        deleteNotes: vi.fn<(notes: number[], signal?: AbortSignal) => Promise<void>>(() => Promise.resolve()),
        guiEditNote: vi.fn<(note: number) => Promise<void>>(() => Promise.resolve()),
        guiBrowse: vi.fn<(query: string) => Promise<number>>(() => Promise.resolve(1)),
    };
    const notices = createCardNotices();
    const cards = useAddToAnki({
        wordItems,
        selectedDict,
        selectedPronunciation,
        aiPick,
        makeSentenceHTML: () => page.sentenceHTML,
        notices,
        services: () => ({ config, ankiService: ankiService as unknown as AnkiService }),
    });
    return { wordItems, selectedDict, selectedPronunciation, aiPick, page, config, ankiService, notices, cards };
}

/** addMarkerNote 第 n 次调用的参数 */
function addNoteCall(ctx: ReturnType<typeof setup>, n = 0) {
    const [deckName, modelName, fields, audio] = ctx.ankiService.addMarkerNote.mock.calls[n];
    return { deckName, modelName, fields, audio };
}

beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(console, 'error').mockImplementation(() => { });
    vi.spyOn(console, 'warn').mockImplementation(() => { });
    mocks.ensureAnkiConnect.mockResolvedValue(undefined);
    mocks.makePronunciationURL.mockResolvedValue({ url: 'https://example.com/fox.mp3', dict: 'youdao' });
    mocks.makePronunciationFilename.mockImplementation((word, type, dict) => Promise.resolve(`${word}_${dict}_${type}.mp3`));
    mocks.dialogMessage.mockResolvedValue(undefined);
});

describe('aiNoteFor', () => {
    const state = (overrides: Partial<AiPickState>): AiPickState => ({
        phase: 'done', pick: { source: 'oxford', index: 2 }, contextualDef: '', note: 'note', fallback: false, ...overrides,
    });

    it('仅在优选已完成且命中所加条目时返回笔记', () => {
        expect(aiNoteFor(state({}), 'oxford', 2)).toBe('note');
        expect(aiNoteFor(state({ phase: 'streaming' }), 'oxford', 2)).toBe(null);
        expect(aiNoteFor(state({}), 'oxford', 1)).toBe(null);
        expect(aiNoteFor(state({}), 'collins', 2)).toBe(null);
        expect(aiNoteFor(state({ pick: null }), 'oxford', 2)).toBe(null);
    });
});

describe('useAddToAnki：添加', () => {
    it('写入内容在点击时快照：等待 Anki 期间界面状态变化不混进这张卡片', async () => {
        const ctx = setup();
        const connect = deferred<void>();
        mocks.ensureAnkiConnect.mockReturnValueOnce(connect.promise);
        const pending = ctx.cards.changeItemAddedOf('collins', 0);
        expect(ctx.wordItems.collins[0].status).toBe('processing-add');

        // 等待期间：用户改了句子、换了发音与牌组、得到了新词的 AI 优选（恰好落在同一序号上）
        ctx.page.sentenceHTML = 'A different <b>sentence</b>.';
        ctx.selectedPronunciation.value = 'en';
        ctx.config.deckName = 'Another Deck';
        ctx.config.modelName = 'Another Model';
        Object.assign(ctx.aiPick, { phase: 'streaming', note: 'half-baked' });

        connect.resolve();
        await pending;

        const call = addNoteCall(ctx);
        // 牌组名留空时按内置默认值生效
        expect(call.deckName).toBe('划词助手默认牌组');
        expect(call.modelName).toBe('Marker Model');
        expect(call.fields['例句']).toBe('The quick <b>fox</b>.');
        expect(decodeHtmlEntities(call.fields['笔记']!)).toBe('sly as a <fox> & co.');
        expect(mocks.makePronunciationURL).toHaveBeenCalledWith('fox', 'us');
        expect(ctx.ankiService.createDeck).toHaveBeenCalledWith('划词助手默认牌组');
        expect(ctx.wordItems.collins[0]).toMatchObject({ status: 'is-added', id: 1001, notice: null });
    });

    it('AI 笔记仅在优选已完成且命中时写入，并经过转义', async () => {
        const ctx = setup();
        await ctx.cards.changeItemAddedOf('collins', 0);
        expect(addNoteCall(ctx, 0).fields['笔记']).toBe('sly as a &lt;fox&gt; &amp; co.');

        // 未命中：加的是另一条
        await ctx.cards.changeItemAddedOf('collins', 1);
        expect('笔记' in addNoteCall(ctx, 1).fields).toBe(false);

        // 命中但仍在流式输出
        ctx.wordItems.collins[0].status = 'not-added';
        ctx.aiPick.phase = 'streaming';
        await ctx.cards.changeItemAddedOf('collins', 0);
        expect('笔记' in addNoteCall(ctx, 2).fields).toBe(false);
    });

    it('发音地址全部不可用时不附带音频', async () => {
        const ctx = setup();
        mocks.makePronunciationURL.mockResolvedValueOnce(null);
        await ctx.cards.changeItemAddedOf('collins', 0);
        expect(addNoteCall(ctx).audio).toBe(null);
        expect(mocks.makePronunciationFilename).not.toHaveBeenCalled();
    });

    it('有可用发音时附带音频地址与文件名', async () => {
        const ctx = setup();
        await ctx.cards.changeItemAddedOf('collins', 0);
        expect(addNoteCall(ctx).audio).toEqual({ url: 'https://example.com/fox.mp3', filename: 'fox_youdao_us.mp3' });
    });

    it('连接失败：回到未添加并弹出错误框，不发出写入', async () => {
        const ctx = setup();
        mocks.ensureAnkiConnect.mockRejectedValueOnce(new Error('connection refused'));
        await ctx.cards.changeItemAddedOf('collins', 0);
        expect(ctx.wordItems.collins[0].status).toBe('not-added');
        expect(mocks.dialogMessage).toHaveBeenCalledWith('Error: connection refused', { title: '无法连接 Anki', kind: 'error' });
        expect(ctx.ankiService.addMarkerNote).not.toHaveBeenCalled();
    });
});

describe('useAddToAnki：写入失败后的对账', () => {
    it('核对发现其实已写入：按已添加处理并接上笔记 id', async () => {
        const ctx = setup();
        ctx.ankiService.addMarkerNote.mockRejectedValueOnce(new Error('timeout'));
        ctx.ankiService.findAddedMarkerNote.mockResolvedValueOnce(4242);
        const before = Date.now();
        await ctx.cards.changeItemAddedOf('collins', 0);

        const [deckName, modelName, fields, since] = ctx.ankiService.findAddedMarkerNote.mock.calls[0];
        expect([deckName, modelName]).toEqual(['划词助手默认牌组', 'Marker Model']);
        expect(fields).toEqual(addNoteCall(ctx).fields);
        // 核对窗口从写入前留出时钟偏差余量
        expect(since).toBeLessThanOrEqual(before);
        expect(ctx.wordItems.collins[0]).toMatchObject({ status: 'is-added', id: 4242, notice: null });
    });

    it('核对确认没有写入：回到未添加，提示可重试，重试重新发起添加', async () => {
        const ctx = setup();
        ctx.ankiService.addMarkerNote.mockRejectedValueOnce(new Error('timeout'));
        await ctx.cards.changeItemAddedOf('collins', 0);
        const item = ctx.wordItems.collins[0];
        expect(item.status).toBe('not-added');
        expect(item.notice).toMatchObject({ severity: 'error', message: '添加失败：timeout', actionLabel: '重试' });

        ctx.notices.runAction(item);
        await vi.waitFor(() => expect(item.status).toBe('is-added'));
        expect(ctx.ankiService.addMarkerNote).toHaveBeenCalledTimes(2);
    });

    it('连核对都失败：状态未知，回到未添加但不给重试', async () => {
        const ctx = setup();
        ctx.ankiService.addMarkerNote.mockRejectedValueOnce(new Error('socket hang up'));
        ctx.ankiService.findAddedMarkerNote.mockRejectedValueOnce(new Error('still down'));
        await ctx.cards.changeItemAddedOf('collins', 0);
        const item = ctx.wordItems.collins[0];
        expect(item.status).toBe('not-added');
        expect(item.notice?.severity).toBe('warning');
        expect(item.notice?.message).toContain('socket hang up');
        expect(item.notice?.actionLabel).toBeUndefined();
    });

    it('addNote 返回 null 也按写入失败对账', async () => {
        const ctx = setup();
        ctx.ankiService.addMarkerNote.mockResolvedValueOnce(null);
        await ctx.cards.changeItemAddedOf('collins', 0);
        expect(ctx.ankiService.findAddedMarkerNote).toHaveBeenCalledOnce();
        expect(ctx.wordItems.collins[0].notice?.message).toBe('添加失败：addNote returns null');
    });
});

describe('useAddToAnki：移除', () => {
    it('已添加的条目再点一次即从 Anki 移除', async () => {
        const ctx = setup();
        Object.assign(ctx.wordItems.collins[0], { status: 'is-added', id: 77 });
        await ctx.cards.changeItemAdded(0);
        expect(ctx.ankiService.deleteNotes.mock.calls[0][0]).toEqual([77]);
        expect(ctx.wordItems.collins[0]).toMatchObject({ status: 'not-added', id: null });
    });

    it('移除失败：保持已添加并提示可重试', async () => {
        const ctx = setup();
        Object.assign(ctx.wordItems.collins[0], { status: 'is-added', id: 77 });
        ctx.ankiService.deleteNotes.mockRejectedValueOnce(new Error('busy'));
        await ctx.cards.changeItemAdded(0);
        expect(ctx.wordItems.collins[0]).toMatchObject({ status: 'is-added', id: 77 });
        expect(ctx.wordItems.collins[0].notice).toMatchObject({ message: '移除失败：busy', actionLabel: '重试' });
    });
});

describe('useAddToAnki：AI 优选卡片入口', () => {
    it('操作 AI 选中的条目，即使它不在当前词典标签页', async () => {
        const ctx = setup();
        ctx.selectedDict.value = 'youdao';
        await ctx.cards.handleAiPickAdd();
        expect(ctx.wordItems.collins[0].status).toBe('is-added');
    });
});
