/**
 * 排版：把词元排成绝对定位的盒子。
 *
 * 为什么自己做排版：并入 / 撤销上下文时，文字会整段重排。交给浏览器流式排版，
 * 我们只能看到“重排前”和“重排后”两个状态，中间没有任何可以插值的东西；到了
 * 动画层就只能整块平移或用裁剪遮罩去“假装”。
 *
 * 这里借的是 Pretext 的两段式思路：测量一次（canvas measureText，带缓存），
 * 之后排版只是对缓存宽度的算术。每个词元因此有明确的 (x, y, w, h)，浏览器只负责
 * 画字，位置、换行、以及位置之间的补间全部由我们决定。
 *
 * 上下文（前后各一到两句）也走同一条流水线：它们不是独立控件里的普通文本，
 * 而是和正文一样平铺的片段，只是角色不同、按块纵向堆叠。这样“正文变高、下文跟着
 * 下移”只是 y 的变化，可以由同一套补间驱动，不会再出现两个排版系统各算各的坐标。
 */

import { breakLines, type BreakAtom } from './sentenceBreak';

export interface LayoutFont {
    /** canvas font 字符串，与正文一致（常规字重） */
    font: string;
    /** canvas font 字符串，与选中时的粗体一致（词元按它预留宽度） */
    boldFont: string;
    /** 行高（px），与 CSS 的 line-height 一致 */
    lineHeight: number;
}

/** 片段属于哪一块：上文、正文、下文 */
export type FragmentRole = 'before' | 'sentence' | 'after';

/** 一个词元盒子（长词被拆行时，同一个 token 会有多个片段） */
export interface LaidFragment {
    /** 稳定 key：跨两次排版保持不变的元素会被复用，left/top 因此可以补间 */
    key: string;
    /** 块内 token 下标（正文用它做 data-token，用于选中与悬停） */
    index: number;
    /** 所属块 */
    role: FragmentRole;
    text: string;
    x: number;
    y: number;
    width: number;
    height: number;
    /** 是否是可选中的单词词元 */
    word: boolean;
}

export interface LayoutResult {
    fragments: LaidFragment[];
    height: number;
}

export type Measure = (text: string, font: string) => number;

interface Item {
    index: number;
    text: string;
    word: boolean;
    space: boolean;
    /** 同一个 token 被拆成多个 item 时的序号（拼 key 用） */
    ordinal: number;
}

const CJK = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;

/** 非词词元（标点、空白、CJK）拆成可换行的最小单位 */
function buildItems(tokens: string[], wordFlags: boolean[], start: number, end: number): Item[] {
    const items: Item[] = [];
    for (let index = start; index < end; index++) {
        const token = tokens[index];
        if (wordFlags[index] === true) {
            items.push({ index, text: token, word: true, space: false, ordinal: 0 });
            continue;
        }
        let ordinal = 0;
        for (const part of token.split(/(\s+)/u)) {
            if (part.length === 0) {
                continue;
            }
            if (/^\s+$/u.test(part)) {
                items.push({ index, text: part, word: false, space: true, ordinal: ordinal++ });
            } else if (CJK.test(part)) {
                // CJK 没有空格可断：按字拆开，换行才能落在字与字之间；
                // 标点串保持完整，避免换行落在逗号、句号前面
                for (const ch of part) {
                    items.push({ index, text: ch, word: false, space: false, ordinal: ordinal++ });
                }
            } else {
                items.push({ index, text: part, word: false, space: false, ordinal: ordinal++ });
            }
        }
    }
    return items;
}

/**
 * 非词盒子不再额外留内边距：位置由排版算好，标点直接贴着前一个词。
 * （原来用 2px padding 是为了让行内盒子相接，现在盒子是绝对定位，不需要补偿）
 */
const PUNCTUATION_PADDING = 0;

/**
 * 排一个块（一段连续的词元），返回片段与块底部的 y。
 *
 * 分两步：先把词元变成“原子”（单词、标点、CJK 单字，以及超过一整行的长词劈开的字素），
 * 标出哪里可以换行、哪里隔着空格；再交给整段最优的断行器（见 sentenceBreak.ts）选出每一行
 * 和这一行的空格宽度；最后按行摆放。只有三种位置可以断行：空白之后、CJK 字之间、长到一行
 * 放不下的词内部。标点永远跟着前一个原子——英文里以标点开头的行是排版错误。
 */
function layoutBlock(
    tokens: string[],
    wordFlags: boolean[],
    keys: number[],
    start: number,
    end: number,
    width: number,
    font: LayoutFont,
    measure: Measure,
    top: number,
    role: FragmentRole,
    keyPrefix: string,
): LayoutResult {
    const items = buildItems(tokens, wordFlags, start, end);
    // 原子与它所属的片段：同一个长词劈出来的相邻原子会在同一行里合并回一个片段
    const atoms: BreakAtom[] = [];
    const owners: { item: Item; text: string }[] = [];
    let afterSpace = false;
    let previousCJK = false;
    let spaceWidth = 0;
    for (const item of items) {
        if (item.space) {
            // 空白一律折成一个空格宽：上下文来自 PDF，句子之间会夹着换行与制表符，
            // 它们的墨宽是 0（或不等宽），按原字符量会让两个词粘在一起（frameworkthat）
            afterSpace = true;
            continue;
        }
        if (spaceWidth === 0) {
            spaceWidth = measure(' ', font.font);
        }
        // 单词按**粗体宽度**预留：这个词随时可能被标记（加粗），盒子必须提前
        // 容得下它，否则选中时会把后面的词挤开。常规字重下字略窄，居中留白
        const unit = item.word
            ? Math.max(measure(item.text, font.font), measure(item.text, font.boldFont))
            : measure(item.text, font.font) + PUNCTUATION_PADDING;
        const breakable = atoms.length > 0
            && (afterSpace || previousCJK || CJK.test(item.text[0] ?? ''));
        if (atoms.length > 0) {
            atoms[atoms.length - 1].spaceAfter = afterSpace;
        }
        afterSpace = false;
        if (unit <= width + 0.01) {
            atoms.push({ width: unit, breakBefore: breakable, spaceAfter: false });
            owners.push({ item, text: item.text });
        } else {
            // 比一整行还宽（URL、超长单词）：按字素拆开，行内任意字素之间都可以换行
            let first = true;
            for (const ch of segmentGraphemes(item.text)) {
                const chWidth = item.word
                    ? Math.max(measure(ch, font.font), measure(ch, font.boldFont))
                    : measure(ch, font.font);
                atoms.push({
                    width: chWidth,
                    breakBefore: first ? breakable : true,
                    spaceAfter: false,
                    inner: !first,
                });
                owners.push({ item, text: ch });
                first = false;
            }
        }
        previousCJK = CJK.test(item.text[item.text.length - 1] ?? '');
    }

    const lines = breakLines(atoms, width, spaceWidth, undefined);
    const fragments: LaidFragment[] = [];
    const ordinals = new Map<Item, number>();
    lines.forEach((line, lineIndex) => {
        const y = top + lineIndex * font.lineHeight;
        let x = 0;
        let pending: { item: Item; text: string; left: number; right: number } | null = null;
        const flush = (): void => {
            if (pending == null) {
                return;
            }
            const ordinal = ordinals.get(pending.item) ?? 0;
            ordinals.set(pending.item, ordinal + 1);
            fragments.push({
                key: keyPrefix + keys[pending.item.index] + ':' + pending.item.ordinal + ':' + ordinal,
                index: pending.item.index,
                role,
                text: pending.text,
                x: pending.left,
                y,
                width: pending.right - pending.left,
                height: font.lineHeight,
                word: pending.item.word,
            });
            pending = null;
        };
        for (let k = line.start; k < line.end; k++) {
            if (k > line.start && atoms[k - 1].spaceAfter) {
                x += line.gap;
            }
            const owner = owners[k];
            if (pending != null && pending.item === owner.item) {
                pending.text += owner.text;
                pending.right = x + atoms[k].width;
            } else {
                flush();
                pending = { item: owner.item, text: owner.text, left: x, right: x + atoms[k].width };
            }
            x += atoms[k].width;
        }
        flush();
    });
    return { fragments, height: lines.length === 0 ? top : top + lines.length * font.lineHeight };
}

/** 排版整句：一整段连续文本，所有词元在同一条文字流里换行 */
export function layoutSentence(
    tokens: string[],
    wordFlags: boolean[],
    keys: number[],
    width: number,
    font: LayoutFont,
    measure: Measure,
): LayoutResult {
    if (width <= 0 || tokens.length === 0) {
        return { fragments: [], height: 0 };
    }
    return layoutBlock(
        tokens, wordFlags, keys, 0, tokens.length, width, font, measure, 0, 'sentence', '',
    );
}

/** 纵向堆叠的一块：正文或某一句上下文 */
export interface StackBlock {
    role: FragmentRole;
    /** 片段 key 的前缀：同一段文字跨两次排版要给同一个前缀，元素才会被复用 */
    keyPrefix: string;
    /** 块内词元（上下文整句作为一个词元传入） */
    tokens: string[];
    words: boolean[];
    keys: number[];
    /** 与上一块之间的间距（px） */
    gap?: number;
}

/** 一块在纵向上占据的范围 */
export interface StackRegion {
    role: FragmentRole;
    top: number;
    bottom: number;
}

export interface StackLayoutResult {
    fragments: LaidFragment[];
    height: number;
    regions: StackRegion[];
}

/**
 * 按块纵向堆叠：每块从新的一行开始，块与块之间留 gap。
 *
 * 块之间不裁切：上下文和正文走的是同一次排版，每个字都有坐标，句子不会在中途断掉。
 * 一块占多高由它自己的文字决定，下一块就从它的底部继续。
 */
export function layoutStack(
    blocks: StackBlock[],
    width: number,
    font: LayoutFont,
    measure: Measure,
): StackLayoutResult {
    const fragments: LaidFragment[] = [];
    const regions: StackRegion[] = [];
    let y = 0;
    for (const block of blocks) {
        if (block.tokens.length === 0 || width <= 0) {
            continue;
        }
        if (regions.length > 0) {
            y += block.gap ?? 0;
        }
        const top = y;
        const laid = layoutBlock(
            block.tokens, block.words, block.keys, 0, block.tokens.length,
            width, font, measure, top, block.role, block.keyPrefix,
        );
        fragments.push(...laid.fragments);
        regions.push({ role: block.role, top, bottom: laid.height });
        y = laid.height;
    }
    return { fragments, height: y, regions };
}

interface GraphemeSegmenter {
    segment(input: string): Iterable<{ segment: string }>;
}
type SegmenterConstructor = new (
    locale?: string,
    options?: { granularity: 'grapheme' },
) => GraphemeSegmenter;

/** Intl.Segmenter 在部分 TS lib 里还没有类型；运行时没有就退到按码点切 */
const SegmenterClass = (Intl as unknown as { Segmenter?: SegmenterConstructor }).Segmenter;
const graphemeSegmenter =
    SegmenterClass == null ? null : new SegmenterClass(undefined, { granularity: 'grapheme' });

function segmentGraphemes(text: string): string[] {
    if (graphemeSegmenter == null) {
        return Array.from(text);
    }
    return Array.from(graphemeSegmenter.segment(text), part => part.segment);
}
