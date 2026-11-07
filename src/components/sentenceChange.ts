/**
 * 判断这次文本变化属于哪一种：并入了上下文、被撤销掉一段，还是整句被替换。
 *
 * 面板的动效全靠这个判断——它一旦分错类，动画就整段失效（曾经因为“缩短”被上游
 * 提前 return 掉，撤销一直没有动画）。所以把它抽成纯函数并单独测。
 */
export type SentenceChange =
    /** 尾部追加：返回追加的字符数 */
    | { kind: 'append'; added: number }
    /** 头部前插：返回前插的字符数 */
    | { kind: 'prepend'; added: number }
    /** 头部被删（撤销一次“并入上文”） */
    | { kind: 'shrink-start'; removed: number }
    /** 尾部被删（撤销一次“并入下文”） */
    | { kind: 'shrink-end'; removed: number }
    /** 整句换掉：重新取词、粘贴、编辑确认 */
    | { kind: 'replace' };

export function classifySentenceChange(previous: string, next: string): SentenceChange {
    if (previous.length === 0 || previous === next) {
        return { kind: 'replace' };
    }
    if (next.length > previous.length) {
        if (next.startsWith(previous)) {
            return { kind: 'append', added: next.length - previous.length };
        }
        if (next.endsWith(previous)) {
            return { kind: 'prepend', added: next.length - previous.length };
        }
        return { kind: 'replace' };
    }
    if (previous.startsWith(next)) {
        return { kind: 'shrink-end', removed: previous.length - next.length };
    }
    if (previous.endsWith(next)) {
        return { kind: 'shrink-start', removed: previous.length - next.length };
    }
    return { kind: 'replace' };
}

/**
 * 词元 key 的对齐：合并 / 撤销只在一端增减文本，另一端的词元仍是“同一批字”。
 * 按字符起点对齐，元素因此能被复用，left/top 才有东西可以补间；整句替换则不复用。
 *
 * 接缝处的那个词元会因为空白归属变化而改写文本：句末的 "." 并入下文后变成 ". "
 * （句号把后面的空格一起收进同一个非词词元）。同一字符起点上允许“一方是另一方的前缀”，
 * 否则它会拿到新 key，被当成新词重播一遍入场动画——正文的句号会从下文那一行飞回来。
 */
export function remapTokenKeys(
    oldTexts: string[],
    oldKeys: number[],
    newTexts: string[],
    change: SentenceChange,
    nextKey: () => number,
): number[] {
    if (change.kind === 'replace') {
        return newTexts.map(() => nextKey());
    }
    const delta = change.kind === 'prepend'
        ? change.added
        : change.kind === 'shrink-start'
            ? -change.removed
            : 0;
    const starts = new Map<number, number>();
    let offset = 0;
    newTexts.forEach((text, index) => {
        starts.set(offset, index);
        offset += text.length;
    });
    const keys = new Array<number>(newTexts.length).fill(-1);
    let oldOffset = 0;
    oldTexts.forEach((text, index) => {
        const target = oldOffset + delta;
        oldOffset += text.length;
        const next = starts.get(target);
        const matched = next != null && (newTexts[next] === text
            || newTexts[next].startsWith(text)
            || text.startsWith(newTexts[next]));
        if (matched) {
            keys[next] = oldKeys[index];
        }
    });
    return keys.map(key => (key >= 0 ? key : nextKey()));
}
