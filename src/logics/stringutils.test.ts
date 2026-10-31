import { afterEach, describe, expect, it, vi } from 'vitest';

import { decodeHtmlEntities, escapeHTML, isWord, rebuildTokensPreservingMarks, tokenize } from './stringutils';

type Token = { token: string; marked: boolean; };

/** 按 tokenize 分词，并标记 markedWords 中出现的单词词元（按词元原文精确匹配） */
function tokensOf(text: string, markedWords: string[] = []): Token[] {
    return tokenize(text).map(token => ({ token, marked: markedWords.includes(token) }));
}

function joined(tokens: { token: string; }[]): string {
    return tokens.map(({ token }) => token).join('');
}

function markedWords(tokens: Token[]): string[] {
    return tokens.filter(({ marked }) => marked).map(({ token }) => token);
}

afterEach(() => {
    vi.restoreAllMocks();
});

describe('tokenize', () => {
    // 文本完整性：词元拼回去必须与原文逐字一致，句子面板与加卡例句都依赖这一点
    it.each([
        '',
        'The quick brown fox jumps over the lazy dog.',
        '  leading and trailing  ',
        "Don't split well-being or rock’n’roll.",
        "'quoted' U.S.A. e.g. 3.14",
        '中文与 English 混排，标点。',
        'café naïve Übermensch',
        'tabs\tand\nnewlines\r\n',
        '😀 emoji 👍🏽 test',
        'a--b - c',
    ])('拼回文本与原文一致：%j', text => {
        expect(joined(tokenize(text).map(token => ({ token })))).toBe(text);
    });

    it('词内撇号与连字符连成单个词元，引号与句点不连接', () => {
        expect(tokenize("don't well-being 'hi'")).toEqual(["don't", ' ', 'well-being', " '", 'hi', "'"]);
        expect(isWord("don't")).toBe(true);
        expect(isWord('U.S.A')).toBe(false);
        expect(isWord('中文')).toBe(false);
    });
});

describe('rebuildTokensPreservingMarks', () => {
    it('没有标记时直接返回新分词，文本完整', () => {
        const sentence = 'A completely different sentence!';
        const rebuilt = rebuildTokensPreservingMarks(tokensOf('old text'), sentence);
        expect(joined(rebuilt)).toBe(sentence);
        expect(markedWords(rebuilt)).toEqual([]);
    });

    it('插入、删除单词后，其余已标记单词按 LCS 对齐保留', () => {
        const old = tokensOf('The quick brown fox jumps over the lazy dog.', ['quick', 'lazy']);
        const sentence = 'The very quick fox happily jumps over the dog lazy.';
        const rebuilt = rebuildTokensPreservingMarks(old, sentence);
        expect(joined(rebuilt)).toBe(sentence);
        expect(markedWords(rebuilt)).toEqual(['quick', 'lazy']);
    });

    it('只改大小写不丢标记', () => {
        const old = tokensOf('the fox runs', ['the']);
        const rebuilt = rebuildTokensPreservingMarks(old, 'The fox runs');
        expect(joined(rebuilt)).toBe('The fox runs');
        expect(markedWords(rebuilt)).toEqual(['The']);
    });

    it('被改写的单词（fox→foxes）经模糊对齐转移标记，差异过大的不转移', () => {
        const rebuilt = rebuildTokensPreservingMarks(tokensOf('the fox runs', ['fox']), 'the foxes runs');
        expect(markedWords(rebuilt)).toEqual(['foxes']);
        const replaced = rebuildTokensPreservingMarks(tokensOf('the elephant runs', ['elephant']), 'the cat runs');
        expect(joined(replaced)).toBe('the cat runs');
        expect(markedWords(replaced)).toEqual([]);
    });

    it('被删除的已标记单词标记随之丢失，文本完整', () => {
        const rebuilt = rebuildTokensPreservingMarks(tokensOf('keep drop keep', ['drop']), 'keep keep');
        expect(joined(rebuilt)).toBe('keep keep');
        expect(markedWords(rebuilt)).toEqual([]);
    });

    it('首尾空白与标点原样保留', () => {
        const sentence = '  “Hello,” she said —  twice.  ';
        const rebuilt = rebuildTokensPreservingMarks(tokensOf('Hello she said', ['said']), sentence);
        expect(joined(rebuilt)).toBe(sentence);
        expect(markedWords(rebuilt)).toEqual(['said']);
    });

    it('剥掉公共前后缀后中段仍超过 LCS 上限：跳过精确对齐，文本完整，前后缀标记保留', () => {
        const warn = vi.spyOn(console, 'warn').mockImplementation(() => { });
        const words = (prefix: string, count: number) => Array.from({ length: count }, (_, i) => `${prefix}${i}`).join(' ');
        // 中段约 2200×2200 个词元（含空白词元），超过 4,000,000 个单元
        const oldText = `head marked ${words('x', 1100)} tail end`;
        const newText = `head marked ${words('q', 1100)} tail end`;
        const old = tokensOf(oldText, ['marked', 'end']);
        const rebuilt = rebuildTokensPreservingMarks(old, newText);
        expect(warn).toHaveBeenCalledOnce();
        expect(joined(rebuilt)).toBe(newText);
        expect(markedWords(rebuilt)).toEqual(['marked', 'end']);
    });

    it('长文本的局部编辑：剥掉前后缀后规模允许，仍走精确对齐', () => {
        const warn = vi.spyOn(console, 'warn').mockImplementation(() => { });
        const words = (prefix: string, count: number) => Array.from({ length: count }, (_, i) => `${prefix}${i}`).join(' ');
        const body = words('w', 3000);
        const oldText = `${body} middle target ${body}`;
        const newText = `${body} middle inserted target ${body}`;
        const rebuilt = rebuildTokensPreservingMarks(tokensOf(oldText, ['target']), newText);
        expect(warn).not.toHaveBeenCalled();
        expect(joined(rebuilt)).toBe(newText);
        expect(markedWords(rebuilt)).toEqual(['target']);
    });
});

describe('escapeHTML / decodeHtmlEntities', () => {
    it.each([
        '<script>alert("XSS")</script>',
        'A & B < C > D',
        "it's “quoted”",
        '&amp; already encoded',
        '',
    ])('转义后再解码与原文一致：%j', text => {
        expect(decodeHtmlEntities(escapeHTML(text))).toBe(text);
    });

    it('转义特殊字符', () => {
        expect(escapeHTML('<b>A & B</b>')).toBe('&lt;b&gt;A &amp; B&lt;/b&gt;');
    });
});
