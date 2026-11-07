/**
 * 断行：整段最优（Knuth–Plass 式动态规划），右边缘靠有界的词间距伸缩收齐。
 *
 * 为什么不用贪心：贪心每行只管“尽量多放”，不看后面的行，右边缘是被一个个放不下的长词
 * 啃出来的。浏览器的 text-wrap: pretty（Safari 整段、Chrome 只管最后四行）、TeX 与
 * typeset 的做法都是同一个思路：把整段当作一个优化问题，给每种断法打分，选总分最低的。
 *
 * 一行的“拉伸比” r 沿用 TeX 的定义：行内多出（或缺少）的宽度，除以所有空格能提供的
 * 伸缩量。r = 0 表示空格正好是自然宽度，r = 1 表示拉满、r = -1 表示压到极限。
 * 坏度 b = 100·|r|³，一行的代价是 (LINE_PENALTY + b)²：松紧越极端代价增长越快，
 * 所以最优解宁可多折一行也不会留下特别松或特别紧的行；相邻两行松紧反差大也要罚，
 * 否则会出现一行很松、下一行很紧的“呼吸感”。
 *
 * 与 TeX 不同的地方：这里没有连字符，放不下时不能靠断词救场，所以“拉伸过头”不是不可行，
 * 只是代价继续增长，实际拉伸被钳在 maxStretch，剩下的缺口留成一点点右侧不齐。
 */

export interface BreakAtom {
    /** 原子宽度（px）：单词按粗体预留宽度，标点、CJK 单字是各自的宽度 */
    width: number;
    /** 在它之前允许换行（与前一个原子之间的空格或 CJK 边界） */
    breakBefore: boolean;
    /** 它与下一个原子之间隔着一个（折叠后的）空格 */
    spaceAfter: boolean;
    /** 在它之前换行会把一个词劈成两半（超过一整行的 URL、长词）：能不劈就不劈 */
    inner?: boolean;
}

export interface BreakTuning {
    /** 空格能拉伸的量：自然宽度的多少倍（TeX 为 1/2） */
    stretch: number;
    /** 空格能压缩的量：自然宽度的多少倍（TeX 为 1/3） */
    shrink: number;
    /** 实际允许的最大拉伸比 r：超过它不再拉，缺口留作右侧不齐 */
    maxStretch: number;
    /** 每多一行的固定代价（TeX 的 \linepenalty）：避免为了凑齐而无谓增加行数 */
    linePenalty: number;
    /** 相邻两行松紧差异的惩罚系数（乘以 r 差的平方） */
    contrast: number;
    /** 末行只有一个词时的惩罚（避免孤字成行） */
    orphan: number;
    /** 没有空格可伸缩的行（CJK、超长词）：缺口占行宽的这个比例时坏度为 100 */
    ragged: number;
    /** 把一个词劈成两半的代价 */
    split: number;
}

export const DEFAULT_TUNING: BreakTuning = {
    stretch: 0.5,
    shrink: 1 / 3,
    maxStretch: 1.2,
    linePenalty: 10,
    contrast: 600,
    orphan: 2000,
    ragged: 0.1,
    split: 50000,
};

export interface BreakLine {
    /** 原子区间 [start, end) */
    start: number;
    end: number;
    /** 这一行每个空格的实际宽度 */
    gap: number;
    /** 末行：不拉伸，空格取自然宽度 */
    last: boolean;
}

interface Edge {
    start: number;
    end: number;
    cost: number;
    ratio: number;
    gap: number;
    last: boolean;
    total: number;
    prev: Edge | null;
}

/** 超宽行（单个不可拆的原子比行还宽）的代价：高到只在别无选择时才会被选中 */
const OVERFULL = 1e9;
/** 比这个小的宽度差当作 0：浮点累加误差不应该决定一行放不放得下 */
const EPSILON = 1e-6;

export function breakLines(
    atoms: BreakAtom[],
    width: number,
    spaceWidth: number,
    tuning: BreakTuning = DEFAULT_TUNING,
): BreakLine[] {
    const count = atoms.length;
    if (count === 0) {
        return [];
    }

    // 前缀和：行 [i, j) 的内容宽度 = 原子宽度之和 + 行内空格（第 j-1 个原子后的空格不算）
    const widthSum = new Float64Array(count + 1);
    const spaceSum = new Int32Array(count + 1);
    for (let index = 0; index < count; index++) {
        widthSum[index + 1] = widthSum[index] + atoms[index].width;
        spaceSum[index + 1] = spaceSum[index] + (atoms[index].spaceAfter ? 1 : 0);
    }
    const spacesIn = (start: number, end: number): number => spaceSum[end - 1] - spaceSum[start];
    const naturalOf = (start: number, end: number): number =>
        widthSum[end] - widthSum[start] + spacesIn(start, end) * spaceWidth;

    const positions: number[] = [0];
    for (let index = 1; index < count; index++) {
        if (atoms[index].breakBefore) {
            positions.push(index);
        }
    }
    positions.push(count);

    // 以每个断点为终点的所有边：每条边是“最后一行 [start, end)”下的最优前缀
    const incoming = new Map<number, Edge[]>();
    incoming.set(0, []);

    const lineCost = (start: number, end: number): { cost: number; ratio: number; gap: number; last: boolean } => {
        const natural = naturalOf(start, end);
        const spaces = spacesIn(start, end);
        const last = end === count;
        const slack = width - natural;
        const squeezed = (): { cost: number; ratio: number; gap: number; last: boolean } => {
            const ratio = spaces === 0 ? -2 : slack / (spaces * spaceWidth * tuning.shrink);
            if (ratio < -1 - EPSILON) {
                return { cost: OVERFULL - slack, ratio: -1, gap: spaceWidth * (1 - tuning.shrink), last };
            }
            const badness = 100 * Math.pow(-ratio, 3);
            return {
                cost: last ? badness : Math.pow(tuning.linePenalty + badness, 2),
                ratio,
                gap: spaceWidth * (1 + ratio * tuning.shrink),
                last,
            };
        };
        if (slack < -EPSILON) {
            return squeezed();
        }
        if (last) {
            // 末行：自然间距，只罚孤字
            const alone = spaces === 0 && start > 0;
            return { cost: alone ? tuning.orphan : 0, ratio: 0, gap: spaceWidth, last: true };
        }
        // 拉伸的坏度封顶在最大拉伸；拉满之后剩下的缺口是右侧不齐，按它占行宽的比例另算。
        // 这样“空格拉得太开”和“根本没有空格可拉”落在同一把尺子上，不会出现一边代价爆炸一边没事
        const stretchRoom = spaces * spaceWidth * tuning.stretch;
        const ratio = stretchRoom > 0 ? Math.min(tuning.maxStretch, slack / stretchRoom) : 0;
        const residual = Math.max(0, slack - ratio * stretchRoom);
        const badness = 100 * Math.pow(ratio, 3) + 100 * Math.pow(residual / (width * tuning.ragged), 3);
        return {
            cost: Math.pow(tuning.linePenalty + badness, 2),
            ratio,
            gap: spaceWidth * (1 + ratio * tuning.stretch),
            last: false,
        };
    };

    for (let from = 0; from < positions.length - 1; from++) {
        const start = positions[from];
        const prevEdges = incoming.get(start);
        if (prevEdges == null || (start !== 0 && prevEdges.length === 0)) {
            continue;
        }
        for (let to = from + 1; to < positions.length; to++) {
            const end = positions[to];
            const natural = naturalOf(start, end);
            const spaces = spacesIn(start, end);
            const minimum = natural - spaces * spaceWidth * tuning.shrink;
            const fits = minimum <= width + EPSILON;
            // 一个断点都放不下：只好整个原子单占一行（超宽），否则这条路径就断了
            if (!fits && to > from + 1) {
                break;
            }
            const line = lineCost(start, end);
            if (end < count && atoms[end].inner === true) {
                line.cost += tuning.split;
            }
            let best: Edge | null = null;
            if (start === 0) {
                best = {
                    start, end, cost: line.cost, ratio: line.ratio, gap: line.gap, last: line.last, total: line.cost, prev: null,
                };
            } else {
                for (const prev of prevEdges) {
                    const jump = prev.last || line.last ? 0 : tuning.contrast * Math.pow(prev.ratio - line.ratio, 2);
                    const total = prev.total + line.cost + jump;
                    if (best == null || total < best.total) {
                        best = {
                            start, end, cost: line.cost, ratio: line.ratio, gap: line.gap, last: line.last, total, prev,
                        };
                    }
                }
            }
            if (best != null) {
                const list = incoming.get(end) ?? [];
                list.push(best);
                incoming.set(end, list);
            }
            if (!fits) {
                break;
            }
        }
    }

    const finals = incoming.get(count) ?? [];
    let chosen: Edge | null = null;
    for (const edge of finals) {
        if (chosen == null || edge.total < chosen.total) {
            chosen = edge;
        }
    }
    const lines: BreakLine[] = [];
    for (let edge: Edge | null = chosen; edge != null; edge = edge.prev) {
        lines.push({ start: edge.start, end: edge.end, gap: edge.gap, last: edge.end === count });
    }
    return lines.reverse();
}
