import { isWord } from '../../logics/stringutils';
import type { SentenceToken, TokenSelection } from './types';

/** 把两个端点归一成区间（含首尾）：拖选时焦点可能落在锚点左侧 */
export function normalizeSelection(a: number, b: number): TokenSelection {
    return a <= b ? { start: a, end: b } : { start: b, end: a };
}

/** 两个选区是否是同一段（含都为空） */
export function isSameSelection(a: TokenSelection | null, b: TokenSelection | null): boolean {
    if (a == null || b == null) {
        return a == null && b == null;
    }
    return a.start === b.start && a.end === b.end;
}

/** 词元是否落在选区内 */
export function selectionContains(selection: TokenSelection | null, index: number): boolean {
    return selection != null && index >= selection.start && index <= selection.end;
}

/**
 * 两段选区是否相交（至少共享一个词元）。相交说明是同一段选区在伸缩，位移可以补间；
 * 不相交说明“另起了一段”，应当淡出淡入而不是滑动
 */
export function selectionsOverlap(a: TokenSelection, b: TokenSelection): boolean {
    return a.start <= b.end && b.start <= a.end;
}

/** 选区覆盖的单词词元（标点与空白跳过，顺序即句中顺序） */
export function selectionWords(tokens: SentenceToken[], selection: TokenSelection | null): string[] {
    if (selection == null) {
        return [];
    }
    const words: string[] = [];
    for (let index = selection.start; index <= selection.end && index < tokens.length; index++) {
        const token = tokens[index].token;
        if (isWord(token)) {
            words.push(token);
        }
    }
    return words;
}

/** 快送的查询文本：选中的单词以单空格拼接（有损：丢掉原始标点与空白） */
export function selectionQuery(tokens: SentenceToken[], selection: TokenSelection | null): string {
    return selectionWords(tokens, selection).join(' ');
}

/**
 * 从“被标记的单词词元”还原一段选区：取标记词的包围区间。
 *
 * 只用在编辑后重建分词（rebuildDraft）这条路上：按词元对齐结束后，标记可能散落成几段，
 * 而选区模型只允许一段，因此收拢成它们之间的这一段（原选区是个词组时，“整段保留”比
 * 丢掉一部分更接近用户的预期）
 */
export function selectionFromMarks(tokens: { token: string; marked: boolean; }[]): TokenSelection | null {
    let start = -1;
    let end = -1;
    tokens.forEach(({ token, marked }, index) => {
        if (!marked || !isWord(token)) {
            return;
        }
        if (start < 0) {
            start = index;
        }
        end = index;
    });
    return start < 0 ? null : { start, end };
}
