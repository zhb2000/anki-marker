import { describe, expect, it } from 'vitest';

import { decodeHtmlEntities, tokenize } from '../../logics/stringutils';
import { rebuildDraftTokens, tokensToSentenceHTML } from './sentenceDraft';
import type { SentenceToken } from './types';

function tokensOf(text: string, marked: string[] = []): SentenceToken[] {
    return tokenize(text).map(token => ({ token, marked: marked.includes(token) }));
}

function joined(tokens: SentenceToken[]): string {
    return tokens.map(({ token }) => token).join('');
}

describe('rebuildDraftTokens', () => {
    // 文本完整性：确认编辑后的分词拼回去，必须与草稿 trim 后逐字一致
    it.each([
        'The quick brown fox jumps over the lazy dog.',
        'The quick brown fox jumps over the lazy dog.   ',
        '\n  The quick, “brown” fox — jumps!\t',
        'The quick brown fox 跳过了 the lazy dog。',
        '',
        '   ',
    ])('拼回文本与 trim 后的草稿一致：%j', draft => {
        const baseline = tokensOf('The quick brown fox jumps over the lazy dog.', ['fox', 'lazy']);
        expect(joined(rebuildDraftTokens(baseline, draft))).toBe(draft.trim());
    });

    it('保留基线中仍然存在的标记', () => {
        const baseline = tokensOf('The quick brown fox jumps over the lazy dog.', ['fox', 'lazy']);
        const rebuilt = rebuildDraftTokens(baseline, 'A quick fox jumps over the very lazy dog.  ');
        expect(rebuilt.filter(t => t.marked).map(t => t.token)).toEqual(['fox', 'lazy']);
    });

    it('不修改基线分词（基线可被多次用于草稿预览）', () => {
        const baseline = tokensOf('one two three', ['two']);
        const snapshot = JSON.stringify(baseline);
        rebuildDraftTokens(baseline, 'zero one two three four');
        expect(JSON.stringify(baseline)).toBe(snapshot);
    });
});

describe('tokensToSentenceHTML', () => {
    it('标记的词加粗，逐词元转义', () => {
        const tokens = tokensOf('Use <b> & “quotes” here', ['quotes']);
        expect(tokensToSentenceHTML(tokens)).toBe('Use &lt;b&gt; &amp; “<b>quotes</b>” here');
    });

    // 文本完整性：例句 HTML 去掉加粗标签并解码后，必须与句子逐字一致
    it.each([
        'Plain sentence.',
        'A <script>alert(1)</script> & B',
        '  spaces  kept  ',
        "it's well-being 中文",
    ])('渲染回文本与句子一致：%j', text => {
        const tokens = tokensOf(text, tokenize(text).filter((_, i) => i % 2 === 0));
        const html = tokensToSentenceHTML(tokens);
        expect(decodeHtmlEntities(html.replace(/<\/?b>/g, ''))).toBe(text);
    });
});
