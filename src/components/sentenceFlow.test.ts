import { describe, expect, it } from 'vitest';
import { layoutSentence, layoutStack, type LayoutFont, type Measure } from './sentenceLayout';
import {
    flowPhase,
    flowTiming,
    pieceOffset,
    pieceOpacity,
    planFlow,
    trailingAnchors,
    trailingY,
    type FlowPlan,
    type WordBox,
} from './sentenceFlow';

const font: LayoutFont = { font: 'regular', boldFont: 'bold', lineHeight: 10 };
const measure: Measure = text => text.length;
const WIDTH = 30;
const CONTEXT = 'one two three four five six seven eight nine ten eleven twelve';

function words(text: string): { tokens: string[]; flags: boolean[] } {
    const tokens = text.split(/(\s+)/u).filter(token => token.length > 0);
    return { tokens, flags: tokens.map(token => /\w/u.test(token)) };
}

function box(fragment: { key: string; text: string; x: number; y: number; width: number }): WordBox {
    return { key: fragment.key, text: fragment.text, x: fragment.x, y: fragment.y, width: fragment.width };
}

/** 合并布局里并入的那段词，与独立布局里的下文词 */
function scenario(intro = 'aaa bbb. '): { merged: WordBox[]; standalone: WordBox[]; plan: FlowPlan } {
    const head = words(intro);
    const context = words(CONTEXT);
    const tokens = [...head.tokens, ...context.tokens];
    const laid = layoutSentence(
        tokens, [...head.flags, ...context.flags], tokens.map((_, index) => index), WIDTH, font, measure,
    );
    const merged = laid.fragments.filter(fragment => fragment.index >= head.tokens.length).map(box);
    const stacked = layoutStack(
        [
            { role: 'sentence', keyPrefix: 's:', tokens: [intro.trim()], words: [false], keys: [0] },
            { role: 'after', keyPrefix: 'a:', tokens: [CONTEXT], words: [false], keys: [0], gap: 4 },
        ],
        WIDTH, font, measure,
    );
    const standalone = stacked.fragments.filter(fragment => fragment.role === 'after').map(box);
    const plan = planFlow(merged, standalone, font.lineHeight, WIDTH, text => text.length)!;
    return { merged, standalone, plan };
}

const visibleText = (word: WordBox): boolean => word.text.trim().length > 0;

describe('planFlow：两个方向的文字不能跳变', () => {
    it('追加的起点：合并侧的词在 (h=0,v=0) 时正好落在独立布局里同一个字的位置', () => {
        const { merged, standalone, plan } = scenario();
        const byText = new Map(standalone.filter(visibleText).map(word => [word.text, word]));
        let checked = 0;
        for (const word of merged.filter(visibleText)) {
            const piece = plan.merged.get(word.key)!;
            if (piece.kind !== 'stay') {
                continue;
            }
            const offset = pieceOffset(piece, { h: 0, v: 0 });
            const old = byText.get(word.text)!;
            expect([word.x + offset.x, word.y + offset.y]).toEqual([old.x, old.y]);
            checked += 1;
        }
        expect(checked).toBeGreaterThan(0);
    });

    it('撤销的起点：独立侧的词在 (h=1,v=1) 时正好落在合并布局里同一个字的位置', () => {
        const { merged, standalone, plan } = scenario();
        const byText = new Map(merged.filter(visibleText).map(word => [word.text, word]));
        let checked = 0;
        for (const word of standalone.filter(visibleText)) {
            const piece = plan.standalone.get(word.key)!;
            if (piece.kind !== 'stay') {
                continue;
            }
            const offset = pieceOffset(piece, { h: 1, v: 1 });
            const old = byText.get(word.text)!;
            expect([word.x + offset.x, word.y + offset.y]).toEqual([old.x, old.y]);
            checked += 1;
        }
        expect(checked).toBeGreaterThan(0);
    });

    it('行末放不下的字与下一行行首的字是同一批：一边滑出右界，一边从左界滑入', () => {
        const { merged, standalone, plan } = scenario();
        const arriving = merged.filter(word => plan.merged.get(word.key)?.kind === 'edge').map(word => word.text);
        const leaving = standalone.filter(word => plan.standalone.get(word.key)?.kind === 'edge').map(word => word.text);
        expect(arriving.length).toBeGreaterThan(0);
        expect(arriving).toEqual(leaving);
    });

    it('从左界入场的字在起点完全落在界外，不会先在行首闪现', () => {
        const { merged, plan } = scenario();
        for (const word of merged) {
            const piece = plan.merged.get(word.key);
            if (piece?.kind === 'edge') {
                expect(word.x + word.width + pieceOffset(piece, { h: 0, v: 1 }).x - word.x).toBeLessThanOrEqual(0);
            }
        }
    });

    it('向右滑出的字紧接在本行带子的尾部，不压到留下的字', () => {
        const { standalone, plan } = scenario();
        for (const word of standalone) {
            const piece = plan.standalone.get(word.key);
            if (piece?.kind !== 'edge') {
                continue;
            }
            for (const other of standalone) {
                const stay = plan.standalone.get(other.key);
                if (other.y === word.y && stay?.kind === 'stay') {
                    expect(word.x + piece.dx).toBeGreaterThanOrEqual(other.x + stay.dx + other.width);
                }
            }
        }
    });

    it('两边文字对不上时放弃规划，交给通用动画', () => {
        const { merged, standalone } = scenario();
        expect(planFlow(merged, standalone.slice(1), font.lineHeight, WIDTH, text => text.length)).toBeNull();
    });
});

describe('flowPhase：追加先行内重排再上拼，撤销先下分离再行内重排', () => {
    const timing = flowTiming(40);
    const samples = Array.from({ length: 201 }, (_, index) => (index / 200) * timing.total);

    const times = (direction: 'append' | 'undo', pick: (phase: { h: number; v: number }) => boolean): number[] =>
        samples.filter(t => pick(flowPhase(direction, t, timing)));

    it('任何时刻行内重排与纵向位移都不会同时在动', () => {
        for (const direction of ['append', 'undo'] as const) {
            for (const t of samples) {
                const { h, v } = flowPhase(direction, t, timing);
                const horizontalMoving = h > 0 && h < 1;
                const verticalMoving = v > 0 && v < 1;
                expect(horizontalMoving && verticalMoving).toBe(false);
            }
        }
    });

    it('追加：行内重排全部结束之后，才开始纵向上拼', () => {
        const horizontal = times('append', ({ h }) => h < 1);
        const vertical = times('append', ({ v }) => v > 0);
        expect(Math.max(...horizontal)).toBeLessThan(Math.min(...vertical));
    });

    it('撤销：纵向分离全部结束之后，才开始行内重排', () => {
        const vertical = times('undo', ({ v }) => v > 0);
        const horizontal = times('undo', ({ h }) => h < 1);
        expect(Math.max(...vertical)).toBeLessThan(Math.min(...horizontal));
    });

    it('起止状态：追加从独立走到合并，撤销从合并走回独立', () => {
        expect(flowPhase('append', 0, timing)).toEqual({ h: 0, v: 0 });
        expect(flowPhase('append', timing.total, timing)).toEqual({ h: 1, v: 1 });
        expect(flowPhase('undo', 0, timing)).toEqual({ h: 1, v: 1 });
        expect(flowPhase('undo', timing.total, timing)).toEqual({ h: 0, v: 0 });
    });
});

describe('同时可见的词在任何时刻互不相交（两端对齐的整段最优断行下，宿主与移动方一起）', () => {
    /** 可重复的伪随机：词长、段落长度、容器宽度都在变，才覆盖得到各种断点变化 */
    function rng(seed: number): () => number {
        let state = seed;
        return () => (state = (state * 1664525 + 1013904223) >>> 0) / 2 ** 32;
    }
    const random = rng(20261107);
    const paragraphText = (words: number): string =>
        Array.from({ length: words }, () => 'x'.repeat(1 + Math.floor(random() * 9))).join(' ');
    // 字宽各不相同：宽度 = 每个字符 (charCode % 5 + 3)，空格 4
    const varied: Measure = text => (text === ' ' ? 4 : Array.from(text).reduce((sum, ch) => sum + (ch.charCodeAt(0) % 5) + 3, 0));
    const tokenizeText = (text: string): string[] => text.split(/(\s+)/u).filter(token => token.length > 0);
    const isWordToken = (token: string): boolean => /\w/u.test(token);

    function scenarioOf(width: number, host: string, mover: string) {
        const hostTokens = tokenizeText(host);
        const moverTokens = tokenizeText(mover);
        const all = [...tokenizeText(host + ' '), ...moverTokens];
        const mergedLaid = layoutSentence(
            all, all.map(isWordToken), all.map((_, index) => index), width, font, varied,
        );
        const headCount = tokenizeText(host + ' ').length;
        const stacked = layoutStack(
            [
                {
                    role: 'sentence', keyPrefix: 's:', tokens: hostTokens, words: hostTokens.map(isWordToken),
                    keys: hostTokens.map((_, index) => index),
                },
                {
                    role: 'after', keyPrefix: 'a:', tokens: moverTokens, words: moverTokens.map(isWordToken),
                    keys: moverTokens.map((_, index) => index), gap: 4,
                },
            ],
            width, font, varied,
        );
        const mm = (list: typeof mergedLaid.fragments): WordBox[] => list.map(box);
        const hostPlan = planFlow(
            mm(mergedLaid.fragments.filter(f => f.index < headCount)),
            mm(stacked.fragments.filter(f => f.role === 'sentence')),
            font.lineHeight, width, text => varied(text, ''), { standalone: 1, merged: 1 },
        );
        const moverPlan = planFlow(
            mm(mergedLaid.fragments.filter(f => f.index >= headCount)),
            mm(stacked.fragments.filter(f => f.role === 'after')),
            font.lineHeight, width, text => varied(text, ''),
        );
        return {
            hostPlan, moverPlan,
            merged: mm(mergedLaid.fragments),
            standalone: mm(stacked.fragments),
            headCount, mergedFragments: mergedLaid.fragments, standaloneFragments: stacked.fragments,
        };
    }

    it('追加与撤销的每个时刻，同一行上同时可见的词框不重叠（含宿主被重新断行、多折出一行的情形）', () => {
        let planned = 0;
        let collisions = 0;
        let rowChanges = 0;
        for (let round = 0; round < 260; round += 1) {
            const width = 90 + Math.floor(random() * 170);
            const built = scenarioOf(width, paragraphText(2 + Math.floor(random() * 12)), paragraphText(3 + Math.floor(random() * 22)));
            if (built.hostPlan == null || built.moverPlan == null) {
                continue;
            }
            planned += 1;
            const { hostPlan, moverPlan } = built;
            if ([...hostPlan.merged.values(), ...moverPlan.merged.values()].some(piece => piece.kind === 'edge')) {
                rowChanges += 1;
            }
            const timing = flowTiming(Math.max(hostPlan.maxShift, moverPlan.maxShift));
            const hostMerged = built.mergedFragments.filter(f => f.index < built.headCount).map(box);
            const moverMerged = built.mergedFragments.filter(f => f.index >= built.headCount).map(box);
            const hostStandalone = built.standaloneFragments.filter(f => f.role === 'sentence').map(box);
            const moverStandalone = built.standaloneFragments.filter(f => f.role === 'after').map(box);
            for (const direction of ['append', 'undo'] as const) {
                for (let step = 0; step <= 30; step += 1) {
                    const phase = flowPhase(direction, (step / 30) * timing.total, timing);
                    const boxes: { x0: number; x1: number; y: number }[] = [];
                    const place = (word: WordBox, piece: ReturnType<FlowPlan['merged']['get']>): void => {
                        if (!visibleText(word) || piece == null || pieceOpacity(piece, phase) < 0.15) {
                            return;
                        }
                        const offset = pieceOffset(piece, phase);
                        boxes.push({ x0: word.x + offset.x, x1: word.x + offset.x + word.width, y: word.y + offset.y });
                    };
                    // 屏幕上同时有的元素：新排版的全部词，加上旧排版里需要离场的边缘词
                    for (const [plan, mergedWords, standaloneWords] of [
                        [hostPlan, hostMerged, hostStandalone],
                        [moverPlan, moverMerged, moverStandalone],
                    ] as const) {
                        const real = direction === 'append' ? mergedWords : standaloneWords;
                        const ghost = direction === 'append' ? standaloneWords : mergedWords;
                        const realPieces = direction === 'append' ? plan.merged : plan.standalone;
                        const ghostPieces = direction === 'append' ? plan.standalone : plan.merged;
                        real.forEach(word => place(word, realPieces.get(word.key)));
                        ghost.forEach(word => {
                            const piece = ghostPieces.get(word.key);
                            place(word, piece?.kind === 'edge' ? piece : undefined);
                        });
                    }
                    for (let i = 0; i < boxes.length; i += 1) {
                        for (let j = i + 1; j < boxes.length; j += 1) {
                            const same = Math.abs(boxes[i].y - boxes[j].y) < font.lineHeight / 2;
                            const overlap = Math.min(boxes[i].x1, boxes[j].x1) - Math.max(boxes[i].x0, boxes[j].x0);
                            if (same && overlap > 1.5) {
                                collisions += 1;
                            }
                        }
                    }
                }
            }
        }
        expect(collisions).toBe(0);
        // 扫描范围里必须真有“词换了行”的情形，否则这条测试没有覆盖到它
        expect(planned).toBeGreaterThan(150);
        expect(rowChanges).toBeGreaterThan(5);
    });

    it('两端对齐让宿主在并入前后也会变：末行从自然间距变成中间行，必须纳入规划才不会在第 0 帧跳一下', () => {
        let moved = 0;
        for (let round = 0; round < 60; round += 1) {
            const width = 90 + Math.floor(random() * 170);
            const built = scenarioOf(width, paragraphText(6 + Math.floor(random() * 8)), paragraphText(6));
            const hostMerged = built.mergedFragments.filter(f => f.index < built.headCount && visibleText(box(f)));
            const hostStandalone = built.standaloneFragments.filter(f => f.role === 'sentence');
            if (hostMerged.some((word, index) => Math.abs(word.x - hostStandalone[index].x) > 0.01
                || Math.abs(word.y - hostStandalone[index].y) > 0.01)) {
                moved += 1;
            }
        }
        expect(moved).toBeGreaterThan(30);
    });

    it('起点：宿主和移动方的每个词在 (h=0,v=0) 都落在它们并入前的位置上', () => {
        for (let round = 0; round < 60; round += 1) {
            const width = 90 + Math.floor(random() * 170);
            const built = scenarioOf(width, paragraphText(4 + Math.floor(random() * 8)), paragraphText(5 + Math.floor(random() * 8)));
            if (built.hostPlan == null || built.moverPlan == null) {
                continue;
            }
            const hostMerged = built.mergedFragments.filter(f => f.index < built.headCount).map(box);
            const hostStandalone = built.standaloneFragments.filter(f => f.role === 'sentence').map(box).filter(visibleText);
            const stays = hostMerged.filter(word => visibleText(word) && built.hostPlan!.merged.get(word.key)?.kind === 'stay');
            expect(stays.length).toBeGreaterThan(0);
            for (const word of stays) {
                const piece = built.hostPlan.merged.get(word.key)!;
                const offset = pieceOffset(piece, { h: 0, v: 0 });
                const target = [word.x + offset.x, word.y + offset.y];
                // 并入前它在独立布局里的某个位置：同一行里某个同文字的词
                expect(hostStandalone.some(old => Math.abs(old.x - target[0]) < 1e-6 && Math.abs(old.y - target[1]) < 1e-6)).toBe(true);
            }
        }
    });
});

describe('planFlow：词换行的两个方向与放弃规划的条件', () => {
    const wb = (key: string, text: string, x: number, y: number, width = 4): WordBox => ({ key, text, x, y, width });
    const measureText = (text: string): number => text.length;

    it('被提到上一行的词（-1）：旧位置向左滑出，新位置从右界滑入，且不压到同一行留下的词', () => {
        // 独立：第 0 行 aaaa，第 1 行 bbbb cccc；合并后 bbbb 被提到第 0 行（aaaa bbbb），第 1 行只剩 cccc
        const standalone = [wb('s1', 'aaaa', 0, 0), wb('s2', 'bbbb', 0, 10), wb('s3', 'cccc', 5, 10)];
        const merged = [wb('m1', 'aaaa', 0, 0), wb('m2', 'bbbb', 5, 0), wb('m3', 'cccc', 0, 10)];
        const plan = planFlow(merged, standalone, 10, 20, measureText, { standalone: 1, merged: 1 })!;
        expect(plan).not.toBeNull();
        // bbbb：独立侧在第 1 行行首，向左滑出；合并侧在第 0 行末尾，从右界外滑入
        expect(plan.standalone.get('s2')).toMatchObject({ kind: 'edge' });
        expect(plan.standalone.get('s2')!.dx).toBeLessThan(0);
        expect(plan.merged.get('m2')).toMatchObject({ kind: 'edge' });
        const start = merged[1].x + pieceOffset(plan.merged.get('m2')!, { h: 0, v: 0 }).x;
        expect(start).toBeGreaterThanOrEqual(20);
        // 离场副本：行首的词向左出界后右端不再可见
        const gone = standalone[1].x + pieceOffset(plan.standalone.get('s2')!, { h: 1, v: 1 }).x + standalone[1].width;
        expect(gone).toBeLessThanOrEqual(0);
    });

    it('有词要跨两行以上时放弃规划，交给通用动画', () => {
        const standalone = [wb('s1', 'aaaa', 0, 0), wb('s2', 'bbbb', 0, 10), wb('s3', 'cccc', 0, 20)];
        const merged = [wb('m1', 'aaaa', 0, 0), wb('m2', 'bbbb', 5, 0), wb('m3', 'cccc', 10, 0)];
        expect(planFlow(merged, standalone, 10, 40, measureText)).toBeNull();
    });
});

describe('并入上文：上文是宿主，原样落在原地，整体随更早的一句被挤下去', () => {
    // 上文单独排在最上面（y=0 起）；合并后它成为正文的开头，更早的一句占了顶部，所以整体下移 H
    const H = 24;
    const old = [
        { key: 'b1', text: 'alpha', x: 0, y: 0, width: 5 },
        { key: 'b2', text: 'beta', x: 6, y: 0, width: 4 },
        { key: 'b3', text: 'gamma', x: 0, y: 10, width: 5 },
    ];
    const head = old.map(word => ({ ...word, key: 'h' + word.key, y: word.y + H }));
    const plan = planFlow(head, old, 10, 30, text => text.length, { standalone: 0.45, merged: 1 })!;

    it('宿主的词按纵向行程 -H 出发，起点正好是它们在上文里的位置', () => {
        expect(plan.drop).toBe(-H);
        for (const word of head) {
            const piece = plan.merged.get(word.key)!;
            const offset = pieceOffset(piece, { h: 0, v: 0 });
            const original = old.find(item => item.text === word.text)!;
            expect([word.x + offset.x, word.y + offset.y]).toEqual([original.x, original.y]);
        }
    });

    it('宿主的词全程只有纵向位移（上文与正文里的字排在同一个位置）', () => {
        for (const word of head) {
            const piece = plan.merged.get(word.key)!;
            for (let step = 0; step <= 10; step += 1) {
                expect(Math.abs(pieceOffset(piece, { h: step / 10, v: step / 10 }).x)).toBeLessThan(1e-9);
            }
        }
    });
});

describe('不透明度：淡色上下文并入后变成正文色', () => {
    const grey = planFlow(
        [{ key: 'm', text: 'word', x: 10, y: 0, width: 4 }],
        [{ key: 's', text: 'word', x: 0, y: 14, width: 4 }],
        10, 30, text => text.length, { standalone: 0.45, merged: 1 },
    )!.merged.get('m')!;
    const solid = planFlow(
        [{ key: 'm', text: 'word', x: 10, y: 0, width: 4 }],
        [{ key: 's', text: 'word', x: 0, y: 14, width: 4 }],
        10, 30, text => text.length, { standalone: 1, merged: 1 },
    )!.merged.get('m')!;
    const timing = flowTiming(40);

    it('淡色 → 正文色：起点是淡色、终点是正文色，途中只升不降（两个方向都一样）', () => {
        for (const direction of ['append', 'undo'] as const) {
            const values = Array.from({ length: 101 }, (_, index) =>
                pieceOpacity(grey, flowPhase(direction, (index / 100) * timing.total, timing)));
            const ordered = direction === 'append' ? values : values.slice().reverse();
            expect(ordered[0]).toBeCloseTo(0.45);
            expect(ordered[100]).toBeCloseTo(1);
            for (let index = 1; index < ordered.length; index += 1) {
                expect(ordered[index]).toBeGreaterThanOrEqual(ordered[index - 1] - 1e-9);
            }
        }
    });

    it('前后都是正文色的词（并入上文时原来的正文）全程不变，不会闪', () => {
        for (let index = 0; index <= 100; index += 1) {
            const phase = flowPhase('append', (index / 100) * timing.total, timing);
            expect(pieceOpacity(solid, phase)).toBe(1);
        }
    });

    it('没有横向行程时起点与终点仍是完整的 (0,0) / (1,1)，不透明度不会在第一帧跳变', () => {
        const still = flowTiming(0);
        for (const direction of ['append', 'undo'] as const) {
            const first = flowPhase(direction, 0, still);
            const last = flowPhase(direction, still.total, still);
            expect(first).toEqual(direction === 'append' ? { h: 0, v: 0 } : { h: 1, v: 1 });
            expect(last).toEqual(direction === 'append' ? { h: 1, v: 1 } : { h: 0, v: 0 });
        }
        expect(pieceOpacity(grey, flowPhase('append', 0, still))).toBeCloseTo(0.45);
    });
});

describe('多出来的一行不与后续下文同时出现在同一处', () => {
    // 独立：aaaa bbbb / cccc dddd（2 行，整体比合并低 14）；合并：    aaaa / bbbb cccc / dddd（3 行）
    const box = (key: string, text: string, x: number, y: number): WordBox => ({ key, text, x, y, width: 4 });
    const standalone = [box('s1', 'aaaa', 0, 14), box('s2', 'bbbb', 5, 14), box('s3', 'cccc', 0, 24), box('s4', 'dddd', 5, 24)];
    const merged = [box('m1', 'aaaa', 10, 0), box('m2', 'bbbb', 0, 10), box('m3', 'cccc', 5, 10), box('m4', 'dddd', 0, 20)];
    const plan = planFlow(merged, standalone, 10, 14, text => text.length)!;

    it('合并布局多出的那一行上的词，在后续下文还在让位 / 归位期间完全不可见', () => {
        const piece = plan.merged.get('m4')!;
        expect(piece.late).toBe(true);
        expect(plan.merged.get('m2')!.late).not.toBe(true);
        for (let h = 0; h <= 0.3; h += 0.05) {
            expect(pieceOpacity(piece, { h, v: 0 })).toBe(0);
        }
        expect(pieceOpacity(piece, { h: 1, v: 0 })).toBeGreaterThan(0.4);
    });

    it('后续下文的让位全部发生在那段区间内：之后它已经停在分离态的位置', () => {
        const anchors = trailingAnchors(30, 22, plan.drop);
        expect(trailingY(anchors, { h: 0.31, v: 0 })).toBe(anchors.held);
    });
});

describe('trailingY：后续下文不能压到落下来的字', () => {
    it('分离态（行内已按合并排、纵向未拼接）下，它在落下来的那段之后', () => {
        const drop = 14;
        const anchors = trailingAnchors(30, 22, drop);
        expect(trailingY(anchors, { h: 1, v: 0 })).toBeGreaterThanOrEqual(22 + drop);
        expect(trailingY(anchors, { h: 0, v: 0 })).toBe(30);
        expect(trailingY(anchors, { h: 1, v: 1 })).toBe(22);
    });
});
