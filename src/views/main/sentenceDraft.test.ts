import { describe, expect, it } from 'vitest';

import { decodeHtmlEntities, tokenize } from '../../logics/stringutils';
import { rebuildDraft, tokensToSentenceHTML } from './sentenceDraft';
import { selectionWords } from './selection';
import type { SentenceToken, TokenSelection } from './types';

function tokensOf(text: string): SentenceToken[] {
    return tokenize(text).map(token => ({ token }));
}

/** 按词元原文定位一段连续选区（含首尾） */
function selectionOf(text: string, first: string, last: string = first): TokenSelection {
    const tokens = tokenize(text);
    const start = tokens.indexOf(first);
    const end = tokens.indexOf(last);
    if (start < 0 || end < start) {
        throw new Error(`无法在句子中定位选区：${first}..${last}`);
    }
    return { start, end };
}

function joined(tokens: SentenceToken[]): string {
    return tokens.map(({ token }) => token).join('');
}

const SENTENCE = 'The quick brown fox jumps over the lazy dog.';

describe('rebuildDraft', () => {
    // 文本完整性：确认编辑后的分词拼回去，必须与草稿 trim 后逐字一致
    it.each([
        'The quick brown fox jumps over the lazy dog.',
        'The quick brown fox jumps over the lazy dog.   ',
        '\n  The quick, “brown” fox — jumps!\t',
        'The quick brown fox 跳过了 the lazy dog。',
        '',
        '   ',
    ])('拼回文本与 trim 后的草稿一致：%j', draft => {
        const baseline = tokensOf(SENTENCE);
        const rebuilt = rebuildDraft(baseline, selectionOf(SENTENCE, 'fox', 'jumps'), draft);
        expect(joined(rebuilt.tokens)).toBe(draft.trim());
    });

    it('选区随句子一起迁移：编辑后仍选中原来那段词', () => {
        const baseline = tokensOf(SENTENCE);
        const rebuilt = rebuildDraft(
            baseline,
            selectionOf(SENTENCE, 'fox', 'jumps'),
            'A quick fox jumps over the very lazy dog.  '
        );
        expect(selectionWords(rebuilt.tokens, rebuilt.selection)).toEqual(['fox', 'jumps']);
    });

    it('没有选区时重建也不产生选区', () => {
        const rebuilt = rebuildDraft(tokensOf(SENTENCE), null, 'A quick fox.');
        expect(rebuilt.selection).toBe(null);
    });

    it('选区中间被删掉一个词时收拢成一段连续区间（选区模型没有两段）', () => {
        const baseline = tokensOf(SENTENCE);
        const rebuilt = rebuildDraft(
            baseline,
            selectionOf(SENTENCE, 'quick', 'fox'),
            'The quick fox jumps over the lazy dog.'
        );
        // 对齐后 quick 与 fox 都还在但已不相邻：取包围区间，仍是一段
        expect(selectionWords(rebuilt.tokens, rebuilt.selection)).toEqual(['quick', 'fox']);
        expect(rebuilt.selection).toEqual({ start: 2, end: 4 });
    });

    it('不修改基线分词与选区（可被多次用于草稿预览）', () => {
        const baseline = tokensOf('one two three');
        const selection: TokenSelection = { start: 2, end: 2 };
        const snapshot = JSON.stringify([baseline, selection]);
        rebuildDraft(baseline, selection, 'zero one two three four');
        expect(JSON.stringify([baseline, selection])).toBe(snapshot);
    });
});

describe('tokensToSentenceHTML', () => {
    it('选区内的单词加粗，逐词元转义', () => {
        const text = 'Use <b> & “quotes” here';
        const tokens = tokensOf(text);
        const selection = selectionOf(text, 'quotes');
        expect(tokensToSentenceHTML(tokens, selection)).toBe('Use &lt;b&gt; &amp; “<b>quotes</b>” here');
    });

    it('选区覆盖的标点跟着覆盖但不加粗', () => {
        const tokens = tokensOf('look, forward');
        expect(tokensToSentenceHTML(tokens, { start: 0, end: 2 })).toBe('<b>look</b>, <b>forward</b>');
    });

    it('没有选区时不加粗', () => {
        expect(tokensToSentenceHTML(tokensOf('a b'), null)).toBe('a b');
    });

    // 文本完整性：例句 HTML 去掉加粗标签并解码后，必须与句子逐字一致
    it.each([
        'Plain sentence.',
        'A <script>alert(1)</script> & B',
        '  spaces  kept  ',
        "it's well-being 中文",
    ])('渲染回文本与句子一致：%j', text => {
        const tokens = tokensOf(text);
        const html = tokensToSentenceHTML(tokens, { start: 0, end: tokens.length - 1 });
        expect(decodeHtmlEntities(html.replace(/<\/?b>/g, ''))).toBe(text);
    });
});
