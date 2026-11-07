/**
 * 并入 / 撤销“下文”的逐词动画：两个方向共用的规划与位姿。
 *
 * 并入下文 = 下文那句话（独立成段）拼进正文末尾（合并）。它们是同一件事的两个方向：
 *
 * - 追加：先行内重排（每行整体右移），停顿，再整体上移一行拼接；
 * - 撤销：先整体下移一行分离，停顿，再行内重排（每行整体左移）。
 *
 * 所以只有两个进度量，位姿是它们的纯函数，方向只决定它们随时间怎么走：
 *   h：行内重排进度，0 = 独立布局的行内位置，1 = 合并布局的行内位置；
 *   v：纵向进度，0 = 独立布局所在的行，1 = 合并布局所在的行。
 * 追加：h 先 0→1，再 v 0→1；撤销：v 先 1→0，再 h 1→0。
 *
 * 每一行是一条刚性的带子。合并布局比独立布局在行首多放了上一行末尾“挤下来”的词，
 * 因此同一批字在一边是行末（向右滑出界外），在另一边是下一行行首（从左侧界外滑入）：
 *
 * - stay：两边都在这一行的词，整行平移；
 * - edge：合并侧是行首被挤下来的词（arrive，从左界外滑入），独立侧是行末放不下的词
 *   （out，向右界外滑出）。
 *
 * 并入“上文”是同一套算法：被并入的上文是宿主（host），原来的正文是移动方（mover）——
 * 旧正文行内重排后上拼到上文末尾，上文文字原地变深。所以规划是按“一组词”做的，
 * 一次动画可以有多组（移动方、宿主），各自带着自己的纵向行程与前后不透明度。
 *
 * 两份布局的词粒度不同（合并布局里标点是单独的词元，独立布局里它跟在词后面），
 * 所以按“非空白字符的累计偏移”对齐，而不是按词一一对应。
 */

export interface WordBox {
    key: string;
    text: string;
    x: number;
    y: number;
    width: number;
}

export interface FlowPiece {
    /** 这个词属于哪份布局（它的元素就停在那份布局的坐标上） */
    side: 'merged' | 'standalone';
    kind: 'stay' | 'edge';
    /** 横向行程：从独立布局的行内位置到合并布局的行内位置要走的距离（恒为正向右） */
    dx: number;
    /** 纵向行程：合并布局整体比独立布局高出的距离（可以为负：上拼的同时整体被上文挤下去） */
    drop: number;
    /** 所在行（在它自己那份布局里的行号）：选区底色按行把同一条带子上的词归在一起 */
    line: number;
    /** 独立状态 / 合并状态下的不透明度：淡色上下文 → 正文色，或始终是正文色 */
    standaloneOpacity: number;
    mergedOpacity: number;
    /**
     * 合并布局比独立布局多出来的行上的词：独立布局里那个位置还是后续下文，
     * 要等后续下文让开之后才能显形（追加）/ 先消失后后续下文才归位（撤销）
     */
    late?: boolean;
}

export interface FlowPlan {
    /** 合并布局整体比独立布局高出的距离（一行 + 块间距）：纵向行程 */
    drop: number;
    merged: Map<string, FlowPiece>;
    standalone: Map<string, FlowPiece>;
    /** 最大的行内行程：决定横向段时长 */
    maxShift: number;
}

interface Span extends WordBox {
    start: number;
    end: number;
    line: number;
}

function visible(word: WordBox): boolean {
    return word.text.trim().length > 0;
}

function toSpans(words: WordBox[], lineHeight: number): { list: Span[]; total: number; top: number } {
    const top = Math.min(...words.map(word => word.y));
    let offset = 0;
    const list = words.map(word => {
        const start = offset;
        offset += word.text.replace(/\s+/gu, '').length;
        return { ...word, start, end: offset, line: Math.round((word.y - top) / lineHeight) };
    });
    return { list, total: offset, top };
}

/**
 * 字符偏移在这份布局里落在哪里：落在某个词内部时，用前缀的真实宽度定位。
 * 按字符数比例估会因为字母宽窄不一而偏几个像素，标点与词会粘在一起。
 */
function locate(
    spans: Span[],
    offset: number,
    measure: (text: string) => number,
): { x: number; line: number } | null {
    const item = spans.find(span => offset >= span.start && offset < span.end);
    if (item == null) {
        return null;
    }
    if (offset === item.start) {
        return { x: item.x, line: item.line };
    }
    return {
        x: item.x + measure(item.text.replace(/\s+/gu, '').slice(0, offset - item.start)),
        line: item.line,
    };
}

/**
 * @param merged 合并布局里这一组的词（按文本顺序）
 * @param standalone 独立布局里这一组的词（按文本顺序）
 * @param boundary 容器宽度：出界 / 进界的参照
 * @param measure 常规字重下一段文字的宽度（定位落在词内部的字符）
 * @param tones 这一组词在独立 / 合并状态下的不透明度
 * @returns 两边文字对不上、或有词跨了不止一行时返回 null，由调用方退回通用动画
 *
 * 不假设断点的单调性：两端对齐 + 整段最优的断行下，合并后断点可能前移也可能后移，
 * 宿主（原样保留的开头）的末几行也会跟着重新分配空格。所以每个词只看一件事——
 * 它在两份布局里的行号差：
 *   0  留下：同一行里平移（带符号的横向位移，可左可右）；
 *   +1 被挤到下一行：旧位置向右滑出右界，新位置从左界滑入；
 *   -1 被提到上一行：旧位置向左滑出左界，新位置从右界滑入。
 * 同一行同方向的出入词共用一个位移，整条带子保持刚性，不会在界外叠成一团。
 */
export function planFlow(
    merged: WordBox[],
    standalone: WordBox[],
    lineHeight: number,
    boundary: number,
    measure: (text: string) => number,
    tones: { standalone: number; merged: number } = { standalone: 0.45, merged: 1 },
): FlowPlan | null {
    const mergedWords = merged.filter(visible);
    const standaloneWords = standalone.filter(visible);
    if (mergedWords.length === 0 || standaloneWords.length === 0 || lineHeight <= 0) {
        return null;
    }
    const m = toSpans(mergedWords, lineHeight);
    const s = toSpans(standaloneWords, lineHeight);
    if (m.total !== s.total) {
        return null;
    }
    const standaloneLines = Math.max(...s.list.map(span => span.line)) + 1;
    const drop = s.top - m.top;
    const make = (
        side: FlowPiece['side'],
        kind: FlowPiece['kind'],
        dx: number,
        line: number,
        late?: boolean,
    ): FlowPiece => ({
        side,
        kind,
        dx,
        drop,
        line,
        late,
        standaloneOpacity: tones.standalone,
        mergedOpacity: tones.merged,
    });

    // 每个独立侧的词对应到合并布局的哪里；每个合并侧的词对应到独立布局的哪里
    interface Link { word: Span; row: number; x: number; shift: number }
    const link = (word: Span, other: Span[]): Link | null => {
        const at = locate(other, word.start, measure);
        if (at == null) {
            return null;
        }
        return { word, row: at.line, x: at.x, shift: Math.abs(at.line - word.line) };
    };
    const fromStandalone: Link[] = [];
    for (const word of s.list) {
        const found = link(word, m.list);
        if (found == null || found.shift > 1) {
            return null;
        }
        fromStandalone.push(found);
    }
    const fromMerged: Link[] = [];
    for (const word of m.list) {
        const found = link(word, s.list);
        if (found == null || found.shift > 1) {
            return null;
        }
        fromMerged.push(found);
    }

    // 每一行留下的词的位移：出入词要跟这条带子保持相对次序
    const firstStay = new Map<number, number>();
    const lastStay = new Map<number, number>();
    for (const item of fromStandalone) {
        if (item.row === item.word.line) {
            const dx = item.x - item.word.x;
            if (!firstStay.has(item.word.line)) {
                firstStay.set(item.word.line, dx);
            }
            lastStay.set(item.word.line, dx);
        }
    }

    // 独立侧：被挤走的词，同一行同方向共用一个位移
    const outForward = new Map<number, number>();
    const outBackward = new Map<number, number>();
    for (const item of fromStandalone) {
        const row = item.word.line;
        if (item.row === row + 1) {
            const need = boundary - item.word.x;
            const own = lastStay.get(row);
            outForward.set(row, Math.max(outForward.get(row) ?? -Infinity, need, own ?? -Infinity));
        } else if (item.row === row - 1) {
            const need = -(item.word.x + item.word.width + 1);
            const own = firstStay.get(row);
            outBackward.set(row, Math.min(outBackward.get(row) ?? Infinity, need, own ?? Infinity));
        }
    }
    const standalonePieces = new Map<string, FlowPiece>();
    for (const item of fromStandalone) {
        const word = item.word;
        const row = word.line;
        if (item.row === row) {
            standalonePieces.set(word.key, make('standalone', 'stay', item.x - word.x, row));
        } else if (item.row === row + 1) {
            standalonePieces.set(word.key, make('standalone', 'edge', outForward.get(row) ?? 0, row));
        } else {
            standalonePieces.set(word.key, make('standalone', 'edge', outBackward.get(row) ?? 0, row));
        }
    }

    // 合并侧：新进来的词，同一行同方向共用一个位移
    const inForward = new Map<number, number>();
    const inBackward = new Map<number, number>();
    for (const item of fromMerged) {
        const row = item.word.line;
        if (item.row === row - 1) {
            const need = item.word.x + item.word.width + 1;
            const own = firstStay.get(row);
            inForward.set(row, Math.max(inForward.get(row) ?? -Infinity, need, own ?? -Infinity));
        } else if (item.row === row + 1) {
            const need = boundary - item.word.x;
            const own = lastStay.get(row);
            inBackward.set(row, Math.max(inBackward.get(row) ?? -Infinity, need, own == null ? -Infinity : -own));
        }
    }
    const mergedPieces = new Map<string, FlowPiece>();
    for (const item of fromMerged) {
        const word = item.word;
        const row = word.line;
        const late = row >= standaloneLines;
        if (item.row === row) {
            mergedPieces.set(word.key, make('merged', 'stay', word.x - item.x, row, late));
        } else if (item.row === row - 1) {
            mergedPieces.set(word.key, make('merged', 'edge', inForward.get(row) ?? 0, row, late));
        } else {
            mergedPieces.set(word.key, make('merged', 'edge', -(inBackward.get(row) ?? 0), row, late));
        }
    }

    let maxShift = 0;
    let anyStay = false;
    for (const piece of [...mergedPieces.values(), ...standalonePieces.values()]) {
        if (piece.kind === 'stay') {
            anyStay = true;
            maxShift = Math.max(maxShift, Math.abs(piece.dx));
        }
    }
    if (!anyStay) {
        for (const piece of [...mergedPieces.values(), ...standalonePieces.values()]) {
            maxShift = Math.max(maxShift, Math.abs(piece.dx));
        }
    }
    return { drop, merged: mergedPieces, standalone: standalonePieces, maxShift };
}

export interface FlowPhase {
    h: number;
    v: number;
}

/** 词元相对它所在布局坐标的位移：合并侧的元素停在 (h=1, v=1)，独立侧的元素停在 (h=0, v=0) */
export function pieceOffset(piece: FlowPiece, phase: FlowPhase): { x: number; y: number } {
    if (piece.side === 'merged') {
        return { x: (phase.h - 1) * piece.dx, y: (1 - phase.v) * piece.drop };
    }
    return { x: phase.h * piece.dx, y: -phase.v * piece.drop };
}

/**
 * 后续下文只在行内重排最靠近“独立布局”的这一小段（h < 30%）里让位 / 归位：
 * 合并布局多出来的那一行要等它让开之后才显形，撤销时先消失它才归位，两者永远不同时出现在同一处。
 */
const TRAILING_LAG = 0.3;
/** 多出来的那一行从完全透明到完全显形，在 h 上占的跨度 */
const LATE_REVEAL = 0.2;

/** 独立侧 out 词在 h 走到一半之后淡出（它们正滑向右界，边缘处顺势淡掉） */
const OUT_FADE_FROM = 0.5;
/**
 * 前后不透明度不同的词（淡色上下文 ↔ 正文色）：行内重排只走很小一段，大头留给纵向拼接——
 * 追加里是 0.45 → 0.58（行内重排后）→ 1（拼接后）。
 */
const OPACITY_HORIZONTAL_SHARE = 0.236;

export function pieceOpacity(piece: FlowPiece, phase: FlowPhase): number {
    if (piece.side === 'standalone' && piece.kind === 'edge') {
        const fade = Math.min(1, Math.max(0, (phase.h - OUT_FADE_FROM) / (1 - OUT_FADE_FROM)));
        return piece.standaloneOpacity * (1 - fade);
    }
    const share = OPACITY_HORIZONTAL_SHARE * phase.h + (1 - OPACITY_HORIZONTAL_SHARE) * phase.v;
    const base = piece.standaloneOpacity + (piece.mergedOpacity - piece.standaloneOpacity) * share;
    if (piece.late === true) {
        const reveal = Math.min(1, Math.max(0, (phase.h - TRAILING_LAG) / LATE_REVEAL));
        return base * reveal;
    }
    return base;
}

export interface TrailingAnchors {
    /** 后续下文（紧跟这句之后的其余句子）在独立布局里的 y */
    standalone: number;
    /** 在合并布局里的 y */
    merged: number;
    /** 分离态（纵向未拼接、行内已按合并排）下的 y：不能压到落下来的那段字 */
    held: number;
}

export function trailingAnchors(standalone: number, merged: number, drop: number): TrailingAnchors {
    return { standalone, merged, held: Math.max(standalone, merged + drop) };
}

export function trailingLag(h: number): number {
    return Math.min(1, Math.max(0, h / TRAILING_LAG));
}

export function trailingY(anchors: TrailingAnchors, phase: FlowPhase): number {
    const lag = trailingLag(phase.h);
    return anchors.standalone
        + (anchors.held - anchors.standalone) * lag
        + (anchors.merged - anchors.held) * phase.v;
}

export type FlowDirection = 'append' | 'undo';

export interface FlowTiming {
    horizontal: number;
    hold: number;
    vertical: number;
    total: number;
}

const FLOW_HOLD_MS = 80;
const FLOW_VERTICAL_MS = 160;

export function flowTiming(maxShift: number): FlowTiming {
    const horizontal = maxShift < 1 ? 0 : Math.round(Math.min(460, Math.max(280, 260 + maxShift * 0.8)));
    const hold = horizontal === 0 ? 0 : FLOW_HOLD_MS;
    return {
        horizontal,
        hold,
        vertical: FLOW_VERTICAL_MS,
        total: horizontal + hold + FLOW_VERTICAL_MS,
    };
}

/** CSS cubic-bezier 的 JS 版 */
export function cubicBezier(x1: number, y1: number, x2: number, y2: number): (p: number) => number {
    const axis = (a: number, b: number, s: number): number =>
        3 * a * (1 - s) * (1 - s) * s + 3 * b * (1 - s) * s * s + s * s * s;
    return (p: number): number => {
        if (p <= 0 || p >= 1) {
            return Math.min(1, Math.max(0, p));
        }
        let low = 0;
        let high = 1;
        for (let step = 0; step < 24; step++) {
            const mid = (low + high) / 2;
            if (axis(x1, x2, mid) < p) {
                low = mid;
            } else {
                high = mid;
            }
        }
        return axis(y1, y2, (low + high) / 2);
    };
}

const easeHorizontal = cubicBezier(0.22, 0.61, 0.36, 1);
const easeVertical = cubicBezier(0.35, 0, 0.15, 1);

export function progress(t: number, from: number, to: number): number {
    return to <= from ? 1 : Math.min(1, Math.max(0, (t - from) / (to - from)));
}

/** 时刻 t 的进度量：追加与撤销是同一对量按相反次序走 */
export function flowPhase(direction: FlowDirection, t: number, timing: FlowTiming): FlowPhase {
    const { horizontal, hold, vertical, total } = timing;
    if (direction === 'append') {
        const v = easeVertical(progress(t, horizontal + hold, total));
        // 没有横向行程（整行本来就在原位）：行内进度跟着纵向走，不然起点的不透明度会直接跳到 h=1 的值
        return { h: horizontal === 0 ? v : easeHorizontal(progress(t, 0, horizontal)), v };
    }
    const v = 1 - easeVertical(progress(t, 0, vertical));
    return { v, h: horizontal === 0 ? v : 1 - easeHorizontal(progress(t, vertical + hold, total)) };
}
