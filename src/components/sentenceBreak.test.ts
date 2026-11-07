import { describe, expect, it } from 'vitest';

import { breakLines, DEFAULT_TUNING, type BreakAtom } from './sentenceBreak';

const SPACE = 4;

/** 可重复的伪随机：每个测试用例用固定种子，失败时能原样复现 */
function rng(seed: number): () => number {
    let state = seed;
    return () => (state = (state * 1664525 + 1013904223) >>> 0) / 2 ** 32;
}

/** 一段“英文”：词宽 12~60px，每个词之后隔一个空格 */
function paragraph(random: () => number, words: number): BreakAtom[] {
    return Array.from({ length: words }, (_, index) => ({
        width: 12 + Math.floor(random() * 48),
        breakBefore: index > 0,
        spaceAfter: index < words - 1,
    }));
}

/** 按本行实际 gap 重算的行宽：衡量右边缘到底齐不齐 */
function usedWidth(atoms: BreakAtom[], line: { start: number; end: number; gap: number }): number {
    let width = 0;
    for (let index = line.start; index < line.end; index += 1) {
        width += atoms[index].width;
        if (index < line.end - 1 && atoms[index].spaceAfter) {
            width += line.gap;
        }
    }
    return width;
}

function greedy(atoms: BreakAtom[], width: number): { start: number; end: number }[] {
    const lines: { start: number; end: number }[] = [];
    let start = 0;
    let used = 0;
    atoms.forEach((atom, index) => {
        const next = used + (index > start ? SPACE : 0) + atom.width;
        if (index > start && next > width) {
            lines.push({ start, end: index });
            start = index;
            used = atom.width;
        } else {
            used = next;
        }
    });
    lines.push({ start, end: atoms.length });
    return lines;
}

describe('breakLines：每一行都合法', () => {
    const random = rng(7);
    const cases = Array.from({ length: 120 }, () => ({
        atoms: paragraph(random, 4 + Math.floor(random() * 40)),
        width: 160 + Math.floor(random() * 260),
    }));

    it('所有原子按原顺序恰好出现一次：不丢字、不重复、不空行', () => {
        for (const { atoms, width } of cases) {
            const lines = breakLines(atoms, width, SPACE);
            expect(lines[0].start).toBe(0);
            expect(lines[lines.length - 1].end).toBe(atoms.length);
            lines.forEach((line, index) => {
                expect(line.end).toBeGreaterThan(line.start);
                if (index > 0) {
                    expect(line.start).toBe(lines[index - 1].end);
                }
            });
        }
    });

    it('只在允许换行的位置换行', () => {
        const random2 = rng(11);
        const atoms: BreakAtom[] = Array.from({ length: 60 }, (_, index) => ({
            width: 14 + Math.floor(random2() * 30),
            // 每三个原子里只有第一个之前能换行：后两个是“紧跟前一个”的标点或词内部
            breakBefore: index % 3 === 0 && index > 0,
            spaceAfter: index % 3 === 2,
        }));
        const lines = breakLines(atoms, 200, SPACE);
        for (const line of lines) {
            if (line.start > 0) {
                expect(atoms[line.start].breakBefore).toBe(true);
            }
        }
    });

    it('除非单个原子本身就比一行宽，每行实际宽度不超过行宽', () => {
        for (const { atoms, width } of cases) {
            for (const line of breakLines(atoms, width, SPACE)) {
                if (line.end - line.start > 1) {
                    expect(usedWidth(atoms, line)).toBeLessThanOrEqual(width + 1e-6);
                }
            }
        }
    });

    it('词间距有界：不会拉成一片白，也不会压到粘在一起', () => {
        const { stretch, shrink, maxStretch } = DEFAULT_TUNING;
        for (const { atoms, width } of cases) {
            for (const line of breakLines(atoms, width, SPACE)) {
                expect(line.gap).toBeGreaterThanOrEqual(SPACE * (1 - shrink) - 1e-9);
                expect(line.gap).toBeLessThanOrEqual(SPACE * (1 + stretch * maxStretch) + 1e-9);
            }
        }
    });

    it('末行不拉伸：空格取自然宽度，和普通左对齐文本一样', () => {
        for (const { atoms, width } of cases) {
            const lines = breakLines(atoms, width, SPACE);
            const last = lines[lines.length - 1];
            expect(last.last).toBe(true);
            if (usedWidth(atoms, { ...last, gap: SPACE }) <= width) {
                expect(last.gap).toBe(SPACE);
            }
        }
    });

    it('一行装得下的短段落：一行，自然间距', () => {
        const atoms = paragraph(rng(3), 3);
        const lines = breakLines(atoms, 10000, SPACE);
        expect(lines).toEqual([{ start: 0, end: 3, gap: SPACE, last: true }]);
    });

    it('单个原子比一行还宽也不会中断：它单占一行，其余照常排', () => {
        const atoms: BreakAtom[] = [
            { width: 30, breakBefore: false, spaceAfter: true },
            { width: 500, breakBefore: true, spaceAfter: true },
            { width: 30, breakBefore: true, spaceAfter: false },
        ];
        const lines = breakLines(atoms, 200, SPACE);
        expect(lines.map(line => [line.start, line.end])).toEqual([[0, 1], [1, 2], [2, 3]]);
    });
});

describe('breakLines：右边缘收齐（相对贪心）', () => {
    it('非末行的右侧缺口：整体比贪心小得多，最大缺口也更小', () => {
        const random = rng(2024);
        const optimal: number[] = [];
        const greedyGaps: number[] = [];
        for (let round = 0; round < 200; round += 1) {
            const atoms = paragraph(random, 15 + Math.floor(random() * 30));
            const width = 220 + Math.floor(random() * 200);
            const lines = breakLines(atoms, width, SPACE);
            lines.slice(0, -1).forEach(line => optimal.push(width - usedWidth(atoms, line)));
            const plain = greedy(atoms, width);
            plain.slice(0, -1).forEach(line => greedyGaps.push(width - usedWidth(atoms, { ...line, gap: SPACE })));
        }
        const mean = (values: number[]): number => values.reduce((sum, value) => sum + value, 0) / values.length;
        expect(mean(optimal)).toBeLessThan(mean(greedyGaps) / 3);
        expect(Math.max(...optimal)).toBeLessThan(Math.max(...greedyGaps));
    });
});

describe('breakLines：整段最优的一个直观例子', () => {
    // 行宽 6，单位词宽：AAA BB CC DDDDD（维基百科 Knuth–Plass 的例子，这里空格按 1 算）
    it('贪心会把 BB 留在第一行、让第二行空出一大截；最优把 BB 挪到第二行', () => {
        const atoms: BreakAtom[] = [3, 2, 2, 5].map((width, index) => ({
            width,
            breakBefore: index > 0,
            spaceAfter: index < 3,
        }));
        const lines = breakLines(atoms, 6, 1, { ...DEFAULT_TUNING, maxStretch: 0 });
        expect(lines.map(line => [line.start, line.end])).toEqual([[0, 1], [1, 3], [3, 4]]);
    });
});
