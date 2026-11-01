import { describe, expect, it } from 'vitest';

import { tokenize } from '../../logics/stringutils';
import {
    isSameSelection,
    normalizeSelection,
    selectionContains,
    selectionFromMarks,
    selectionQuery,
    selectionsOverlap,
    selectionWords,
} from './selection';
import type { SentenceToken } from './types';

/** look(0) ,(1) forward(2) (3) to(4) (5) it(6) */
const SENTENCE = 'look, forward to it';

function tokensOf(text: string): SentenceToken[] {
    return tokenize(text).map(token => ({ token }));
}

function marksOf(text: string, markedWords: string[]): { token: string; marked: boolean; }[] {
    return tokenize(text).map(token => ({ token, marked: markedWords.includes(token) }));
}

describe('normalizeSelection', () => {
    it('把两个端点归一成含首尾的区间（反向拖选）', () => {
        expect(normalizeSelection(4, 2)).toEqual({ start: 2, end: 4 });
        expect(normalizeSelection(2, 4)).toEqual({ start: 2, end: 4 });
        expect(normalizeSelection(3, 3)).toEqual({ start: 3, end: 3 });
    });
});

describe('isSameSelection', () => {
    it('比较区间（含都为空）', () => {
        expect(isSameSelection(null, null)).toBe(true);
        expect(isSameSelection({ start: 1, end: 2 }, { start: 1, end: 2 })).toBe(true);
        expect(isSameSelection({ start: 1, end: 2 }, { start: 1, end: 3 })).toBe(false);
        expect(isSameSelection(null, { start: 1, end: 1 })).toBe(false);
    });
});

describe('selectionContains', () => {
    it('端点算在选区内，空选区不含任何词元', () => {
        expect(selectionContains({ start: 2, end: 4 }, 2)).toBe(true);
        expect(selectionContains({ start: 2, end: 4 }, 4)).toBe(true);
        expect(selectionContains({ start: 2, end: 4 }, 5)).toBe(false);
        expect(selectionContains(null, 2)).toBe(false);
    });
});

describe('selectionsOverlap', () => {
    it('共享端点即相交（同一段在伸缩），否则是另起一段', () => {
        expect(selectionsOverlap({ start: 0, end: 2 }, { start: 2, end: 4 })).toBe(true);
        expect(selectionsOverlap({ start: 0, end: 2 }, { start: 3, end: 4 })).toBe(false);
    });
});

describe('selectionWords / selectionQuery', () => {
    it('跳过标点与空白，按句子顺序取单词', () => {
        const tokens = tokensOf(SENTENCE);
        expect(selectionWords(tokens, { start: 0, end: 4 })).toEqual(['look', 'forward', 'to']);
        expect(selectionQuery(tokens, { start: 0, end: 4 })).toBe('look forward to');
    });

    it('空选区没有查询文本', () => {
        expect(selectionQuery(tokensOf(SENTENCE), null)).toBe('');
    });

    it('区间越界时截断在已有词元内', () => {
        const tokens = tokensOf(SENTENCE);
        expect(selectionWords(tokens, { start: 4, end: 99 })).toEqual(['to', 'it']);
    });
});

describe('selectionFromMarks', () => {
    it('取标记词的包围区间：散开的标记也只收拢成一段', () => {
        expect(selectionFromMarks(marksOf(SENTENCE, ['look', 'forward', 'to']))).toEqual({ start: 0, end: 4 });
        expect(selectionFromMarks(marksOf(SENTENCE, ['look', 'it']))).toEqual({ start: 0, end: 6 });
    });

    it('没有标记词（或只标了标点）时没有选区', () => {
        expect(selectionFromMarks(marksOf(SENTENCE, []))).toBe(null);
        expect(selectionFromMarks(marksOf(SENTENCE, [', ']))).toBe(null);
    });
});
