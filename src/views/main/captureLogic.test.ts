import { describe, expect, it, vi } from 'vitest';

import { tokenize } from '../../logics/stringutils';
import {
    captureFailureDialog,
    createCaptureDelivery,
    isSameCapturedPayload,
    markCapturedWord,
    planCapturedSentence,
    type CapturedSentencePayload,
} from './captureLogic';
import type { SentenceToken } from './types';

function payload(overrides: Partial<CapturedSentencePayload> = {}): CapturedSentencePayload {
    return { text: 'The quick brown fox.', word: 'fox', wordOffset: 16, retry: false, ...overrides };
}

function tokensOf(text: string): SentenceToken[] {
    return tokenize(text).map(token => ({ token, marked: false }));
}

function markedOf(tokens: SentenceToken[]): string[] {
    return tokens.filter(t => t.marked).map(t => t.token);
}

describe('planCapturedSentence', () => {
    it('文本只有空白时不处理', () => {
        expect(planCapturedSentence(payload({ text: '  \n ' }), false)).toEqual({ kind: 'empty' });
    });

    it('用户已改动工作区时丢弃迟到的补取结果', () => {
        expect(planCapturedSentence(payload({ retry: true }), true)).toEqual({ kind: 'stale-retry' });
    });

    it('用户未改动时补取结果照常录入；非补取结果不受用户改动影响', () => {
        expect(planCapturedSentence(payload({ retry: true }), false).kind).toBe('apply');
        expect(planCapturedSentence(payload({ retry: false }), true).kind).toBe('apply');
    });

    it('录入时裁剪文本与命中词，文本未被裁剪时保留偏移', () => {
        expect(planCapturedSentence(payload({ word: ' fox ' }), false)).toEqual({
            kind: 'apply', text: 'The quick brown fox.', word: 'fox', wordOffset: 16,
        });
    });

    it('文本被裁剪改短时偏移不再可信', () => {
        const plan = planCapturedSentence(payload({ text: '  The quick brown fox.  ', wordOffset: 18 }), false);
        expect(plan).toEqual({ kind: 'apply', text: 'The quick brown fox.', word: 'fox', wordOffset: null });
    });

    it.each([null, '', '   '])('命中词为 %j 时视为未命中', word => {
        const plan = planCapturedSentence(payload({ word }), false);
        expect(plan.kind === 'apply' && plan.word).toBe(null);
    });
});

describe('createCaptureDelivery', () => {
    function setup() {
        const apply = vi.fn<(p: CapturedSentencePayload) => void>();
        const discardPending = vi.fn<() => void>();
        const delivery = createCaptureDelivery<CapturedSentencePayload>({ apply, discardPending, isSame: isSameCapturedPayload });
        return { apply, discardPending, delivery };
    }

    it('每个事件都取走后端暂存并丢弃', () => {
        const { apply, discardPending, delivery } = setup();
        delivery.onEvent(payload());
        delivery.onEvent(payload({ text: 'Another fox.' }));
        expect(discardPending).toHaveBeenCalledTimes(2);
        expect(apply).toHaveBeenCalledTimes(2);
    });

    it('挂监听后收到的第一个事件与暂存回放的那份相同：跳过，不重复录入', async () => {
        const { apply, delivery } = setup();
        await delivery.onPending(payload());
        delivery.onEvent(payload());
        expect(apply).toHaveBeenCalledTimes(1);
        // 回放记录只用于第一个事件：之后同样内容的新捕获照常录入
        delivery.onEvent(payload());
        expect(apply).toHaveBeenCalledTimes(2);
    });

    it('第一个事件与回放不同时照常录入', async () => {
        const { apply, delivery } = setup();
        await delivery.onPending(payload());
        delivery.onEvent(payload({ retry: true }));
        expect(apply).toHaveBeenCalledTimes(2);
        expect(apply).toHaveBeenLastCalledWith(payload({ retry: true }));
    });

    it('已经收到过事件时不再回放暂存；暂存为空时什么都不做', async () => {
        const { apply, delivery } = setup();
        await delivery.onPending(null);
        expect(apply).not.toHaveBeenCalled();
        delivery.onEvent(payload({ text: 'From event.' }));
        await delivery.onPending(payload({ text: 'Stale pending.' }));
        expect(apply).toHaveBeenCalledTimes(1);
        expect(apply).toHaveBeenCalledWith(payload({ text: 'From event.' }));
    });

    it('失败原因也按同样规则去重', async () => {
        const apply = vi.fn<(p: 'empty' | 'failed') => void>();
        const delivery = createCaptureDelivery<'empty' | 'failed'>({ apply, discardPending: () => { }, isSame: (a, b) => a === b });
        await delivery.onPending('empty');
        delivery.onEvent('empty');
        delivery.onEvent('failed');
        expect(apply.mock.calls).toEqual([['empty'], ['failed']]);
    });
});

describe('markCapturedWord', () => {
    it('按偏移标记真正被选中的那一处', () => {
        const text = 'fox and fox';
        const tokens = tokensOf(text);
        markCapturedWord(tokens, 'fox', 8, text);
        expect(tokens.map(t => t.marked)).toEqual([false, false, false, false, true]);
    });

    it('偏移与文本不符时退回字符串匹配（先精确、再忽略大小写）', () => {
        const text = 'The Fox and the fox';
        const tokens = tokensOf(text);
        markCapturedWord(tokens, 'fox', 0, text);
        expect(tokens.findIndex(t => t.marked)).toBe(tokens.length - 1);
        const caseOnly = tokensOf('The Fox runs');
        markCapturedWord(caseOnly, 'fox', null, 'The Fox runs');
        expect(markedOf(caseOnly)).toEqual(['Fox']);
    });

    it('短语匹配连续的单词词元', () => {
        const text = 'He gave up, then gave it up.';
        const tokens = tokensOf(text);
        markCapturedWord(tokens, 'gave  up', null, text);
        expect(markedOf(tokens)).toEqual(['gave', 'up']);
        expect(tokens.findIndex(t => t.marked)).toBe(2);
    });

    it('复合词元内按连接符整段命中，不做任意子串匹配', () => {
        const text = 'An encoder-decoder model.';
        const tokens = tokensOf(text);
        markCapturedWord(tokens, 'encoder', null, text);
        expect(markedOf(tokens)).toEqual(['encoder-decoder']);
        const none = tokensOf(text);
        markCapturedWord(none, 'code', null, text);
        expect(markedOf(none)).toEqual([]);
    });

    it('选区横跨多个词元时标记所有相交的单词词元', () => {
        const text = 'big brown fox';
        const tokens = tokensOf(text);
        markCapturedWord(tokens, 'own fo', 6, text);
        expect(markedOf(tokens)).toEqual(['brown', 'fox']);
    });
});

describe('captureFailureDialog', () => {
    it('按失败种类给出标题，按平台给出引导', () => {
        expect(captureFailureDialog('empty', 'macos').title).toBe('没有取到文本');
        expect(captureFailureDialog('failed', 'linux').title).toBe('划词录入失败');
        expect(captureFailureDialog('failed', 'macos').message).toContain('辅助功能');
        expect(captureFailureDialog('empty', 'windows').message).toContain('托盘菜单');
        expect(captureFailureDialog('empty', 'linux').message).toContain('AT-SPI');
    });
});
