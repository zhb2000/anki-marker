import { describe, expect, it } from 'vitest';

import { layoutSentence, layoutStack, type LayoutFont, type Measure } from './sentenceLayout';

const font: LayoutFont = { font: 'regular', boldFont: 'bold', lineHeight: 10 };
/** 每个字符 1px：断言就只剩算术 */
const measure: Measure = text => text.length;

function lay(tokens: string[], words: boolean[], width: number) {
    return layoutSentence(tokens, words, tokens.map((_, i) => i + 1), width, font, measure);
}

describe('layoutSentence', () => {
    it('把词元依次排开：词与空格各自占宽，y 不变', () => {
        const { fragments, height } = lay(['aa', ' ', 'bb'], [true, false, true], 20);
        expect(fragments.map(f => [f.text, f.x, f.y])).toEqual([
            ['aa', 0, 0],
            ['bb', 3, 0],
        ]);
        expect(height).toBe(10);
    });

    it('放不下时在空格处换行，且不把空格带到行首', () => {
        const { fragments, height } = lay(['aaaa', ' ', 'bb'], [true, false, true], 6);
        expect(fragments.map(f => [f.text, f.x, f.y])).toEqual([
            ['aaaa', 0, 0],
            ['bb', 0, 10],
        ]);
        expect(height).toBe(20);
    });

    it('标点跟着前一个词走：换行只发生在词之前', () => {
        const { fragments } = lay(['aaaa', ', ', 'bb'], [true, false, true], 7);
        // 逗号不能出现在行首：它仍留在第一行末尾
        expect(fragments.map(f => [f.text, f.x, f.y])).toEqual([
            ['aaaa', 0, 0],
            [',', 4, 0],
            ['bb', 0, 10],
        ]);
    });

    it('超长单词按字素逐段换行', () => {
        const { fragments, height } = lay(['abcdefgh'], [true], 4);
        expect(fragments.map(f => f.text)).toEqual(['abcd', 'efgh']);
        expect(fragments.map(f => [f.x, f.y])).toEqual([[0, 0], [0, 10]]);
        expect(height).toBe(20);
    });

    it('换行等空白折成一个空格宽，不会把两个词粘在一起', () => {
        const { fragments } = layoutSentence(['aaa\nbbb'], [false], [0], 20, font, measure);
        expect(fragments.map(f => [f.text, f.x])).toEqual([['aaa', 0], ['bbb', 4]]);
    });

    it('同一个 token 被拆行时共享 token 下标，但 key 不同', () => {
        const { fragments } = lay(['abcdefgh'], [true], 4);
        expect(fragments.every(f => f.index === 0)).toBe(true);
        expect(new Set(fragments.map(f => f.key)).size).toBe(fragments.length);
    });
});

describe('layoutSentence：断行与两端对齐', () => {
    const words = ['aa', 'bb', 'cc', 'dd', 'ee', 'ff', 'gg', 'hh'];
    const tokens = words.flatMap((word, index) => (index === 0 ? [word] : [' ', word]));
    const flags = tokens.map(token => token.trim().length > 0);

    it('文字完整：所有可见文字按原顺序一个不少、一个不多', () => {
        for (let width = 5; width <= 40; width += 1) {
            const { fragments } = lay(tokens, flags, width);
            expect(fragments.map(f => f.text).join('')).toBe(words.join(''));
        }
    });

    it('非末行的右端贴到行宽（空格在有界范围内伸缩把行收齐），末行是自然间距的左对齐', () => {
        const { fragments } = lay(tokens, flags, 9);
        const lines = new Map<number, typeof fragments>();
        for (const fragment of fragments) {
            lines.set(fragment.y, [...(lines.get(fragment.y) ?? []), fragment]);
        }
        const rows = Array.from(lines.values());
        const last = rows[rows.length - 1];
        for (const row of rows.slice(0, -1)) {
            const edge = row[row.length - 1];
            // 每行三个词、两个空格：自然宽度 8、行宽 9，缺的 1px 要靠把两个空格各拉长 0.5px 补齐
            expect(edge.x + edge.width).toBeCloseTo(9, 5);
        }
        // 末行：词之间是自然的 1px 空格
        for (let index = 1; index < last.length; index += 1) {
            expect(last[index].x - (last[index - 1].x + last[index - 1].width)).toBeCloseTo(1, 5);
        }
    });

    it('CJK 没有空格：换行落在字与字之间，每个片段都在行宽之内', () => {
        const { fragments } = layoutSentence(['中文排版测试句子'], [false], [0], 5, font, text => text.length * 2);
        expect(fragments.map(f => f.text).join('')).toBe('中文排版测试句子');
        for (const fragment of fragments) {
            expect(fragment.x + fragment.width).toBeLessThanOrEqual(5 + 1e-6);
        }
    });

    it('能整放进一行的词不会被劈开；超过一整行的才按字素劈开', () => {
        const fits = lay(['aaa', ' ', 'bbbbbb'], [true, false, true], 8);
        expect(fits.fragments.map(f => f.text)).toEqual(['aaa', 'bbbbbb']);
        const long = lay(['aaa', ' ', 'bbbbbbbbbbbb'], [true, false, true], 8);
        expect(long.fragments.map(f => f.text).join('')).toBe('aaabbbbbbbbbbbb');
        expect(long.fragments.every(f => f.x + f.width <= 8 + 1e-6)).toBe(true);
    });
});

describe('layoutStack', () => {
    const block = (
        role: 'before' | 'sentence' | 'after',
        text: string,
        extra: { gap?: number } = {},
    ) => ({
        role,
        keyPrefix: role + ':',
        tokens: [text],
        words: [false],
        keys: [0],
        ...extra,
    });

    it('上下文和正文排在同一个坐标系里：块依次向下堆叠，间距只在块之间', () => {
        const { fragments, regions } = layoutStack([
            block('before', 'aa'),
            block('sentence', 'bb', { gap: 4 }),
            block('after', 'cc', { gap: 4 }),
        ], 20, font, measure);
        expect(regions).toEqual([
            { role: 'before', top: 0, bottom: 10 },
            { role: 'sentence', top: 14, bottom: 24 },
            { role: 'after', top: 28, bottom: 38 },
        ]);
        expect(fragments.map(f => [f.text, f.role, f.y])).toEqual([
            ['aa', 'before', 0],
            ['bb', 'sentence', 14],
            ['cc', 'after', 28],
        ]);
    });

    it('上下文整句都排进来：折行不截断，下一块接在它的底部', () => {
        const { fragments, regions, height } = layoutStack([
            block('before', 'aaa bbb ccc'),
            block('sentence', 'dd', { gap: 4 }),
        ], 5, font, measure);
        expect(fragments.filter(f => f.role === 'before').map(f => f.text)).toEqual(['aaa', 'bbb', 'ccc']);
        expect(regions[0]).toEqual({ role: 'before', top: 0, bottom: 30 });
        expect(regions[1]).toEqual({ role: 'sentence', top: 34, bottom: 44 });
        expect(height).toBe(44);
    });
});
