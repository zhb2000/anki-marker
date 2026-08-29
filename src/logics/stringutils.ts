export function isWord(token: string): boolean {
    return /^\w+$/.test(token);
}

export function tokenize(text: string): string[] {
    const tokens = [];
    const regex = /\w+/g;
    let lastEnd = 0;
    for (let match = regex.exec(text); match != null; match = regex.exec(text)) {
        const token = match[0];
        const start = match.index;
        if (lastEnd < start) {
            tokens.push(text.slice(lastEnd, start)); // push previous non-word token
        }
        tokens.push(token); // push current word token
        lastEnd = start + token.length;
    }
    if (lastEnd < text.length) {
        tokens.push(text.slice(lastEnd)); // push the last non-word token if exists
    }
    return tokens;
}

/**
 * 手动编辑句子后重建分词时，尽量保留原有已标记（选中）单词的状态。
 *
 * 以新旧词元序列的最长公共子序列（LCS）对齐：编辑造成的插入、删除不影响其余单词的对齐，
 * 对齐上的词元继承旧的标记状态；被删除的单词无法对齐，其标记随之丢失。
 * 对齐比较忽略大小写，因此仅改动大小写（如句首字母）不会丢失标记。
 *
 * 对 LCS 未能对齐的已标记单词，再做一次模糊对齐：编辑时被改写的单词（如 fox→foxes）
 * 仍视为“原来那个词”，把标记转移到相似度最高（相同时取位置最近）且尚未对齐的新单词词元上。
 * 该启发式无法区分词形变化与仅一字之差的换词（如 fox→box，编辑距离同为极小值），
 * 选择转移标记——撤销错误转移的成本（点击取消）与重新标记的成本相当，而转移能额外
 * 覆盖复数/时态/笔误修正等常见场景。
 */
export function rebuildTokensPreservingMarks(
    oldTokens: { token: string; marked: boolean; }[],
    sentence: string
): { token: string; marked: boolean; }[] {
    const newTokens = tokenize(sentence).map(token => ({ token, marked: false }));
    // 没有已标记词元时无需对齐，直接返回全新词元
    if (!oldTokens.some(({ marked }) => marked)) {
        return newTokens;
    }
    const oldLowered = oldTokens.map(({ token }) => token.toLowerCase());
    const newLowered = newTokens.map(({ token }) => token.toLowerCase());
    // lcs[i][j]：oldLowered 前 i 个与 newLowered 前 j 个词元的最长公共子序列长度
    const lcs: number[][] = Array.from({ length: oldTokens.length + 1 },
        () => new Array<number>(newTokens.length + 1).fill(0));
    for (let i = 1; i <= oldTokens.length; i++) {
        for (let j = 1; j <= newTokens.length; j++) {
            lcs[i][j] = oldLowered[i - 1] === newLowered[j - 1]
                ? lcs[i - 1][j - 1] + 1
                : Math.max(lcs[i - 1][j], lcs[i][j - 1]);
        }
    }
    // 从末尾回溯对齐结果：相等的词元互相匹配，并继承旧的标记状态
    const oldMatched = new Array<boolean>(oldTokens.length).fill(false);
    const newMatched = new Array<boolean>(newTokens.length).fill(false);
    let i = oldTokens.length;
    let j = newTokens.length;
    while (i > 0 && j > 0) {
        if (oldLowered[i - 1] === newLowered[j - 1]) {
            newTokens[j - 1].marked = oldTokens[i - 1].marked;
            oldMatched[i - 1] = true;
            newMatched[j - 1] = true;
            i--;
            j--;
        } else if (lcs[i - 1][j] >= lcs[i][j - 1]) {
            i--;
        } else {
            j--;
        }
    }
    // 模糊补救：LCS 未能对齐的已标记单词可能是编辑时被改写（如 fox→foxes），
    // 把它的标记转移到相似度最高且尚未对齐的新单词词元上（一对一，忽略大小写）
    const fuzzyCandidates = newTokens
        .map((_, index) => index)
        .filter(index => !newMatched[index] && !newTokens[index].marked && isWord(newTokens[index].token));
    for (let oi = 0; oi < oldTokens.length; oi++) {
        if (oldMatched[oi] || !oldTokens[oi].marked || !isWord(oldTokens[oi].token)) {
            continue;
        }
        let bestIndex = -1;
        let bestSimilarity = 0;
        let bestDistance = Number.MAX_SAFE_INTEGER;
        for (const index of fuzzyCandidates) {
            if (newTokens[index].marked) {
                continue;
            }
            const similarity = wordSimilarity(oldTokens[oi].token, newTokens[index].token);
            if (similarity < FUZZY_ALIGNMENT_THRESHOLD) {
                continue;
            }
            const distance = Math.abs(oi - index);
            if (similarity > bestSimilarity || (similarity === bestSimilarity && distance < bestDistance)) {
                bestIndex = index;
                bestSimilarity = similarity;
                bestDistance = distance;
            }
        }
        if (bestIndex >= 0) {
            newTokens[bestIndex].marked = true;
            fuzzyCandidates.splice(fuzzyCandidates.indexOf(bestIndex), 1);
        }
    }
    return newTokens;
}

/** 模糊对齐的相似度阈值：低于该值视为不同的单词，不转移标记 */
const FUZZY_ALIGNMENT_THRESHOLD = 0.6;

/**
 * 两个单词的相似度（0 ~ 1）：1 - 编辑距离 / 较长单词的长度，忽略大小写。
 * 例如 fox/foxes 为 0.6，word/words 为 0.8，fox/box 为 0.33。
 */
function wordSimilarity(a: string, b: string): number {
    const lowerA = a.toLowerCase();
    const lowerB = b.toLowerCase();
    if (lowerA === lowerB) {
        return 1;
    }
    const maxLength = Math.max(lowerA.length, lowerB.length);
    if (maxLength === 0) {
        return 1;
    }
    return 1 - levenshteinDistance(lowerA, lowerB) / maxLength;
}

/** 编辑距离（Levenshtein），滚动数组实现 */
function levenshteinDistance(a: string, b: string): number {
    let prev = Array.from({ length: b.length + 1 }, (_, j) => j);
    let curr = new Array<number>(b.length + 1);
    for (let i = 1; i <= a.length; i++) {
        curr[0] = i;
        for (let j = 1; j <= b.length; j++) {
            curr[j] = Math.min(
                prev[j] + 1,
                curr[j - 1] + 1,
                prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1)
            );
        }
        [prev, curr] = [curr, prev];
    }
    return prev[b.length];
}

const escapeDiv = document.createElement('div');

/**
 * 转义 HTML 字符串中的特殊字符。
 * 
 * 示例：`escapeHTML('<script>alert("XSS")</script>')` 返回 `&lt;script&gt;alert(&quot;XSS&quot;)&lt;/script&gt;`
 */
export function escapeHTML(html: string): string {
    escapeDiv.textContent = html; // 使用 textContent 而不是 innerHTML 来避免 HTML 解析
    return escapeDiv.innerHTML; // 获取转义后的 HTML 字符串
}

const decodeTextarea = document.createElement('textarea');

/**
 * 解码 HTML 字符串中的实体字符。
 * 
 * 示例：`decodeHtmlEntities('&lt;script&gt;alert(&quot;XSS&quot;)&lt;/script&gt;')` 返回 `<script>alert("XSS")</script>`
 */
export function decodeHtmlEntities(content: string): string {
    decodeTextarea.innerHTML = content;
    return decodeTextarea.value;
}
