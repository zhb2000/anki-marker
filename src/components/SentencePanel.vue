<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type PropType } from 'vue';

import * as utils from '../logics/utils';
import { classifySentenceChange, remapTokenKeys } from './sentenceChange';
import {
    flowPhase,
    flowTiming,
    pieceOffset,
    pieceOpacity,
    planFlow,
    progress,
    trailingAnchors,
    trailingY,
    type FlowDirection,
    type FlowPiece,
    type FlowPlan,
    type FlowTiming,
    type TrailingAnchors,
    type WordBox,
} from './sentenceFlow';
import {
    layoutStack,
    type LaidFragment,
    type LayoutFont,
    type Measure,
    type StackBlock,
    type StackRegion,
} from './sentenceLayout';

/**
 * 句子面板 = 一段可以选中的文本。
 *
 * 选中不是“逐个词元打标记的集合”，而是文本控件里那种**一段连续选区**：
 *
 * - 单击单词：选中它（替换原有选区）；
 * - 按住拖动：把选区拉成锚点到指针之间的连续词元（类比文本选择，路径无关）；
 * - Shift+按下：以现有选区的固定端为锚点扩展 / 收缩；
 * - 点在选区内：选区收窄到这个词（文本控件里点选区内就是塌陷到落点）；
 * - 再点一次“已选中的单个词”：取消选中——清空只在这里发生，点在空白处什么也不做。
 *
 * 模型里只有“一段”，不连续的选中在类型上就不存在，因此不需要任何限制、校验或提示文案。
 * 这段选区就是查询目标（父组件在手势结束时快送一次）；可发现性交给光标（单词上是 I-beam、
 * 空白处是箭头）、一整块选区底色与“拖选文本”的既有习惯，不用 tooltip 说教。
 *
 * 动效按语义分三种，各自回答一个问题：
 * - 伸缩（点选区内收窄、拖选、Shift 扩展）：边界补间 —— “同一段选区正在变长 / 变短”；
 * - 替换（与上一段不相交）：同一块底色**挪到**新位置 —— “旧的换成了新的”，不是凭空重建；
 * - 取消（再点一次选中的词、整句替换）：**原地淡出**，不位移也不收边 —— 高亮是覆盖在文字上的一层状态，
 *   不是有体积的物体，消失就是“熄灯”；而边界内缩已经被“收窄到某个词”占用，不能复用。
 */
interface PanelToken {
    token: string;
}

/** 选区：词元下标，含首尾；null 表示没有选中任何词 */
interface PanelSelection {
    start: number;
    end: number;
}

/** 选区底色的一块（跨行时每行一块），绝对定位在文本下方 */
interface HighlightRect {
    key: string;
    left: number;
    top: number;
    width: number;
    height: number;
}

interface Box {
    left: number;
    top: number;
    right: number;
    bottom: number;
}

/** 渲染用的片段：在排版结果上标记“入场 / 退场”，供 CSS 动画选择 */
interface DisplayFragment extends LaidFragment {
    enter?: boolean;
    leave?: boolean;
    /** 离场的旧副本：它在旧选区里是被选中的词（要保持加粗，否则起点的字形会变） */
    selected?: boolean;
}

const props = defineProps({
    tokens: {
        type: Array as PropType<PanelToken[]>,
        required: true,
    },
    selection: {
        type: Object as PropType<PanelSelection | null>,
        default: null,
    },
    /**
     * 句子两侧的上下文（近端在前）：每侧只显示第一句，点一下即并入该侧最近的一句。
     * 常驻显示——需要补救的时刻，恰恰是算法以为自己取对了的时候
     */
    contextBefore: {
        type: Array as PropType<string[]>,
        default: () => [],
    },
    contextAfter: {
        type: Array as PropType<string[]>,
        default: () => [],
    },
});

const emit = defineEmits<{
    /** 选区变化（手势进行中逐次上报；写入方是父组件） */
    'update:selection': [value: PanelSelection | null];
    /** 手势结束（单击松手、拖选松手、Shift 区间松手）：父组件据此把选区快送成一次查询 */
    commit: [];
    /** 点了上/下文的淡色文字：请求把该侧最近的一句并进来 */
    'extend-before': [];
    'extend-after': [];
}>();

const bodyEl = ref<HTMLElement>();
const textEl = ref<HTMLElement>();
/** 指针下的单词词元（悬停底色用）；拖选中不显示 */
const hoverIndex = ref<number | null>(null);
/** 按下进行中：鼠标在面板外松开也能收到 mouseup */
const pressed = ref(false);
/** 拖选已经开始（指针离开过按下的词元）：底色跟着指针走，用更短的补间 */
const dragStarted = ref(false);
/** 面板尺寸变化中：重新量的位置直接到位，不补间 */
const resizing = ref(false);
/** 替换中：底色正在挪到新位置（用更长的补间让“移动”可读） */
const moving = ref(false);

/** 拖选的固定端：按下时确定，焦点端跟着指针走，选区恒为两端之间的区间 */
let anchor: number | null = null;
/** 最近一次上报的选区：区分“自己改的”（保留锚点）与“父组件改的”（锚点跟随新选区） */
let emitted: PanelSelection | null = null;
/**
 * 按在“已选中的单个词”上、等着松手才取消的词元。
 * 若随即拖动，那是“从这个词重新拉起选区”，不该先闪一下消失
 */
let pendingToggle: number | null = null;
/**
 * 点击判定阈值（px）：按下到松手的位移超过它就不算“点了一下”，
 * 与系统拖拽阈值（SM_CXDRAG = 4px）一致。没有它的话，按在已选中的词上
 * 手抖几下（始终没进入别的词）也会被当成单击，把它取消掉
 */
const CLICK_SLOP_PX = 4;
/** 本次按下的起点与“是否已超出阈值” */
let pressX = 0;
let pressY = 0;
let pressMoved = false;

const rects = ref<HighlightRect[]>([]);
/** 本轮排版的词元盒子（绝对定位，left/top 由排版给出） */
const laid = ref<DisplayFragment[]>([]);
/** 正在退场的旧词元（撤销时被删掉的那一段正文）：保留在屏幕上淡出 */
const exiting = ref<DisplayFragment[]>([]);
/** 整块排版（正文 + 上下文）的高度：绝对定位的内容靠它撑出滚动范围 */
const textHeight = ref(0);
/** 句子级动画进行中：选区底色跟着同一时长补间，且不重排选区 */
const sentenceMoving = ref(false);
/** 选区底色由逐帧动画直接驱动（词在两段式移动，CSS 补间跟不上）：此时关闭它的过渡 */
const flowRects = ref(false);
/** 每个 token 的稳定 key：跨合并 / 撤销保持不变，元素才会被复用并补间 left/top */
let tokenKeys: number[] = [];
/** 上一份词元文本（判断这次变化是并入、撤销还是替换） */
let previousTokenTexts: string[] = [];
/** 词元 key 的唯一来源 */
let keySeq = 0;
/** 入场 / 退场的方向（±8px）：并入下文从下往上，并入上文从上往下 */
let shift = '8px';
/**
 * 新词的入场延迟。并入上文时旧词要整体下移，先把这条路让出来，新词再浮现；
 * 否则两层文字会在中途叠在一起。并入下文时旧词不动，新词可以立即出现
 */
const enterDelay = ref('0ms');
/** 退场元素清理的定时器 */
let exitTimer: ReturnType<typeof setTimeout> | null = null;
/** 首次排版完成前不做动画（首帧是“上屏”，不该补间） */
let laidOnce = false;
/** 上一轮显示的选区：判断这次是伸缩还是替换 */
let shown: PanelSelection | null = null;
/** 上下文在纵向上占据的范围（由排版给出），渐变与点击区都按它定位 */
const regions = ref<StackRegion[]>([]);

const hoveredIndex = computed(() => {
    const index = hoverIndex.value;
    if (index == null || !isWordIndex(index) || contains(props.selection, index)) {
        return null;
    }
    return index;
});

function normalize(a: number, b: number): PanelSelection {
    return a <= b ? { start: a, end: b } : { start: b, end: a };
}

function isSame(a: PanelSelection | null, b: PanelSelection | null): boolean {
    if (a == null || b == null) {
        return a == null && b == null;
    }
    return a.start === b.start && a.end === b.end;
}

function contains(selection: PanelSelection | null, index: number): boolean {
    return selection != null && index >= selection.start && index <= selection.end;
}

/** 两段选区是否相交：相交即“同一段在伸缩”，不相交即“替换” */
function overlaps(a: PanelSelection, b: PanelSelection): boolean {
    return a.start <= b.end && b.start <= a.end;
}

function isWordIndex(index: number): boolean {
    const token = props.tokens[index];
    return token != null && utils.string.isWord(token.token);
}

/** 指针所在的词元下标：不在词元上（标点、空白、面板空处、面板外）返回 null */
function indexAt(event: MouseEvent): number | null {
    const target = event.target;
    const el = target instanceof Element ? target.closest<HTMLElement>('[data-token]') : null;
    if (el == null || bodyEl.value?.contains(el) !== true) {
        return null;
    }
    const index = Number(el.dataset.token);
    return Number.isInteger(index) ? index : null;
}

function emitSelection(value: PanelSelection | null): void {
    if (isSame(value, emitted)) {
        return;
    }
    emitted = value;
    emit('update:selection', value);
}

/**
 * 量出选区覆盖的底色矩形：直接用排版结果，不再读 DOM。
 *
 * 文字正在补间时，DOM 里的位置是插值中的中间值；量它会得到“底色跟着文字走”的假象，
 * 却在动画结束后停在错误的位置。排版坐标是最终位置，且与文字共用同一条补间曲线，
 * 所以底色与文字始终对齐。
 */
function measureLines(): Box[] {
    const selection = props.selection;
    if (selection == null) {
        return [];
    }
    const boxes: Box[] = [];
    for (const fragment of laid.value) {
        if (fragment.role !== 'sentence') {
            continue;
        }
        if (fragment.index < selection.start || fragment.index > selection.end) {
            continue;
        }
        if (fragment.width < 0.5) {
            continue;
        }
        boxes.push({
            left: fragment.x,
            top: fragment.y,
            right: fragment.x + fragment.width,
            bottom: fragment.y + fragment.height,
        });
    }
    return mergeLines(boxes);
}

function mergeLines(boxes: Box[]): Box[] {
    const lines: Box[] = [];
    for (const box of boxes.sort((a, b) => a.top - b.top || a.left - b.left)) {
        const line = lines.find(l => box.top < l.bottom - 1 && box.bottom > l.top + 1);
        if (line == null) {
            lines.push({ ...box });
            continue;
        }
        line.left = Math.min(line.left, box.left);
        line.right = Math.max(line.right, box.right);
        line.top = Math.min(line.top, box.top);
        line.bottom = Math.max(line.bottom, box.bottom);
    }
    return lines.sort((a, b) => a.top - b.top);
}

/**
 * 给这一轮的行矩形分配 key：key 不变的元素会被 Vue 复用，几何变化才会补间。
 * 句子级动画时按序号复用，底色与文字共用同一条曲线。
 */
function layoutRects(
    lines: Box[],
    mode: 'enter' | 'resize' | 'swap',
    reuse = false,
): HighlightRect[] {
    const previous = rects.value;
    const used = new Set<string>();
    const flying = mode === 'swap' ? previous[0] : undefined;
    return lines.map((line, index) => {
        const replaced = reuse ? previous[index] : undefined;
        let key: string;
        if (replaced != null) {
            key = replaced.key;
        } else if (index === 0 && flying != null) {
            key = flying.key;
        } else {
            const match = previous.find(r => !used.has(r.key) && Math.abs(r.top - line.top) < 1.5);
            key = match?.key ?? 'rect-' + keySeq++;
        }
        used.add(key);
        return {
            key,
            left: line.left,
            top: line.top,
            width: line.right - line.left,
            height: line.bottom - line.top,
        };
    });
}

async function updateRects(): Promise<void> {
    await nextTick();
    // 逐帧动画正在驱动底色：不要用最终位置去覆盖它，收尾时会统一重算
    if (flowRects.value) {
        return;
    }
    const value = props.selection;
    if (value == null) {
        shown = null;
        rects.value = [];
        return;
    }
    const mode = shown == null ? 'enter' : (overlaps(shown, value) ? 'resize' : 'swap');
    shown = value;
    moving.value = mode === 'swap';
    rects.value = layoutRects(measureLines(), mode, sentenceMoving.value);
}

/** 上下文的显示文本：只显示最近的一句，长句由样式裁到一行 */
const contextAbove = computed(() => props.contextBefore[0] ?? '');
/** 下文显示两句：紧邻的下一句，以及它后面那一句 */
const contextAfter = computed(() => props.contextAfter.slice(0, 2).filter(text => text.length > 0));

function onContextClick(side: 'before' | 'sentence' | 'after'): void {
    if (side === 'before') {
        emit('extend-before');
    } else if (side === 'after') {
        emit('extend-after');
    }
}

/** 与 CSS 的 --sentence-duration 一致（面板里的位移、底色、高度共用这个时长） */
const SENTENCE_MOTION_MS = 320;
/** 本次并入下文的 WAAPI 动画（下一动作/卸载时取消） */
let appendAnimations: Animation[] = [];
/** 当前行高（由排版给出）：合体段整体上移的距离 */
let sentenceLineHeight = 0;

function reducedMotion(): boolean {
    return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * 词元宽度用**隐藏的 span** 量，而不是 canvas。
 *
 * canvas 的字体解析和 CSS 不是同一套：WKWebView 里 canvas 不认识 `-apple-system` 这类
 * 关键字，落到的家族和实际渲染的字体差几个百分点。词元盒子按测量值给宽，盒子窄了字就会
 * 溢出、把后面的空格吃掉——正文因为按粗体宽度预留了余量还能扛住，上下文没有预留，于是
 * 出现 frameworkthat 这种粘连。直接用 DOM 量，等于用浏览器马上要渲染的那套字体量，
 * 宽度和盒子永远一致。
 */
const measureCache = new Map<string, number>();
let measureHost: HTMLElement | null = null;
/** 本次排版用的字体：把 Measure 的 font 字符串映射回常规 / 粗体 */
let measuredFont: LayoutFont | null = null;

function ensureMeasureHost(): HTMLElement | null {
    if (measureHost != null && measureHost.isConnected) {
        return measureHost;
    }
    const host = document.createElement('div');
    host.className = 'measure-host';
    host.setAttribute('aria-hidden', 'true');
    textEl.value?.appendChild(host);
    measureHost = host;
    return host;
}

/**
 * 把面板的 scoped 属性抄给新建的节点。
 *
 * 组件样式是 scoped 的，选择器形如 .token[data-v-xxxx]；脚本里 createElement 出来的
 * 节点没有这个属性，.token 的字体与 white-space 一概不生效，量到的是浏览器默认字体
 * ——这正是之前“盒子比字窄、空格被吃掉”的根因。
 */
function adoptScope(el: HTMLElement): void {
    for (const attr of Array.from(textEl.value?.attributes ?? [])) {
        if (attr.name.startsWith('data-v-')) {
            el.setAttribute(attr.name, '');
        }
    }
}

/** 批量量宽：全部挂上去之后才读，只触发一次布局 */
function measureWidths(texts: string[], bold: boolean): Map<string, number> {
    const widths = new Map<string, number>();
    const host = ensureMeasureHost();
    if (host == null || texts.length === 0) {
        return widths;
    }
    const spans = texts.map(text => {
        const span = document.createElement('span');
        span.className = 'token';
        adoptScope(span);
        span.style.fontWeight = bold ? 'bold' : 'normal';
        span.style.left = '0';
        span.style.top = '0';
        span.textContent = text;
        host.appendChild(span);
        return span;
    });
    spans.forEach((span, index) => {
        widths.set(texts[index], span.getBoundingClientRect().width);
    });
    spans.forEach(span => span.remove());
    return widths;
}

function cacheWidths(texts: Iterable<string>, font: string, bold: boolean): void {
    const pending: string[] = [];
    for (const text of texts) {
        if (!measureCache.has(font + '\u0000' + text)) {
            pending.push(text);
        }
    }
    if (pending.length === 0) {
        return;
    }
    const widths = measureWidths(pending, bold);
    for (const text of pending) {
        measureCache.set(font + '\u0000' + text, widths.get(text) ?? 0);
    }
}

/**
 * 预热：一次把这次排版会用到的文字量完。
 * 长单词按字素拆开时还会用到单字，那些留到 miss 时再单独量（很少）。
 */
function primeMeasure(blocks: StackBlock[], font: LayoutFont): void {
    measuredFont = font;
    const regular = new Set<string>([' ']);
    const bold = new Set<string>();
    for (const block of blocks) {
        block.tokens.forEach((token, index) => {
            regular.add(token);
            for (const part of token.split(/(\s+)/u)) {
                if (part.length > 0) {
                    regular.add(part);
                }
            }
            if (block.words[index] === true) {
                bold.add(token);
            }
        });
    }
    cacheWidths(regular, font.font, false);
    cacheWidths(bold, font.boldFont, true);
}

const domMeasure: Measure = (text, font) => {
    const key = font + '\u0000' + text;
    const cached = measureCache.get(key);
    if (cached != null) {
        return cached;
    }
    const bold = measuredFont != null && font === measuredFont.boldFont;
    const width = measureWidths([text], bold).get(text) ?? text.length * 8;
    measureCache.set(key, width);
    return width;
};

/** 从面板样式读排版参数：字体、粗体与行高都以 CSS 为唯一来源 */
function readFont(): LayoutFont | null {
    const host = textEl.value;
    if (host == null) {
        return null;
    }
    const probe = document.createElement('span');
    probe.className = 'token';
    adoptScope(probe);
    probe.textContent = 'A';
    probe.style.position = 'absolute';
    probe.style.visibility = 'hidden';
    host.appendChild(probe);
    const style = getComputedStyle(probe);
    const size = style.fontSize;
    const family = style.fontFamily;
    const lineHeight = Number.parseFloat(style.lineHeight);
    // 粗体字重从 CSS 读，而不写死 700：不同字体家族的 bold 可能与 700 不等价，
    // 量错宽度会让被标记的词超出它的盒子
    probe.style.fontWeight = 'bold';
    const boldWeight = getComputedStyle(probe).fontWeight;
    probe.remove();
    return {
        font: size + ' ' + family,
        boldFont: boldWeight + ' ' + size + ' ' + family,
        lineHeight: Number.isFinite(lineHeight) ? lineHeight : Number.parseFloat(size) * 1.4,
    };
}

/** 上下文块与正文之间的间距（px）：同一条文字流里的两个块，靠这一点留白分开 */
const CONTEXT_GAP = 4;
/** 上下文的静态不透明度（与 CSS 的 .token.context 保持一致）：补间的落点也用它 */
const CONTEXT_OPACITY = 0.45;

/** 用文字内容的哈希做 key 前缀：内容不变则 key 不变，元素才能被复用并补间 */
function hashText(text: string): string {
    let hash = 0x811c9dc5;
    for (let index = 0; index < text.length; index++) {
        hash ^= text.charCodeAt(index);
        hash = Math.imul(hash, 0x01000193);
    }
    return (hash >>> 0).toString(36);
}

/**
 * 按当前宽度重排：正文与上下文走同一条排版流水线，坐标在同一个坐标系里。
 * 这样“正文变高、下文跟着下移”只是同一批片段 y 的变化，动画可以对每个片段直接补间。
 */
function relayout(): boolean {
    const width = textEl.value?.clientWidth ?? 0;
    const font = readFont();
    if (width <= 0 || font == null) {
        return false;
    }
    const texts = props.tokens.map(token => token.token);
    const words = texts.map((_, index) => isWordIndex(index));
    const blocks: StackBlock[] = [];
    if (contextAbove.value.length > 0) {
        // 上文用与正文相同的分词和相同的单词预留宽度排版：它随时可能被并入成为正文的开头，
        // 并入动画要求每个字在并入前后落在同一个位置，换行也必须一致（下文同理，见下）
        const parts = utils.tokenize(contextAbove.value);
        blocks.push({
            role: 'before',
            keyPrefix: 'b:' + hashText(contextAbove.value) + ':',
            tokens: parts,
            words: parts.map(part => utils.string.isWord(part)),
            keys: parts.map((_, index) => index),
        });
    }
    blocks.push({
        role: 'sentence',
        keyPrefix: 's:',
        tokens: texts,
        words,
        keys: tokenKeys,
        gap: blocks.length > 0 ? CONTEXT_GAP : 0,
    });
    // key 只看文字、不看它在队列里的位置：并入一句后下一句上移，元素仍然是同一个，
    // 位置才有东西可以补间。同一句重复出现时用出现次序区分，避免 key 撞车
    const occurrences = new Map<string, number>();
    for (const text of contextAfter.value) {
        const occurrence = occurrences.get(text) ?? 0;
        occurrences.set(text, occurrence + 1);
        // 与上文同理：下文随时可能并入成为正文的结尾，用同样的分词与单词预留宽度排版，
        // 并入前后的间距与换行才一致，整行才能作为一条刚性的带子平移
        const parts = utils.tokenize(text);
        blocks.push({
            role: 'after',
            keyPrefix: 'a:' + hashText(text) + ':' + occurrence + ':',
            tokens: parts,
            words: parts.map(part => utils.string.isWord(part)),
            keys: parts.map((_, index) => index),
            gap: CONTEXT_GAP,
        });
    }
    primeMeasure(blocks, font);
    primeMeasure(blocks, font);
    const result = layoutStack(blocks, width, font, domMeasure);
    laid.value = result.fragments;
    regions.value = result.regions;
    textHeight.value = result.height;
    sentenceLineHeight = font.lineHeight;
    return true;
}

/**
 * 可交互的上下文区域：上文一块，下文**只圈第一句**。
 *
 * 点击并入的永远是最近的一句，把两句话圈成一整块会让人以为整块都能点；
 * 位置取自排版给出的块范围（也就是那几行文字的上下沿），只是一层覆盖，不包文字。
 */
const contextRegions = computed<StackRegion[]>(() => {
    const picked: StackRegion[] = [];
    for (const region of regions.value) {
        if (region.role === 'sentence' || picked.some(item => item.role === region.role)) {
            continue;
        }
        picked.push(region);
    }
    return picked;
});

/** 一帧里一个元素的状态：相对它所在布局坐标的位移与不透明度 */
interface MotionState {
    x: number;
    y: number;
    opacity?: number;
}

/** 逐帧动画的一条轨道：时间（毫秒）→ 状态 */
interface MotionTrack {
    el: HTMLElement;
    at: (t: number) => MotionState;
    /** 播完保留最终样式（退场元素：清掉会弹回原位闪一下，由清理定时器统一移除） */
    keep?: boolean;
    /** 最近一帧画出的状态：选区底色按它跟随 */
    last?: MotionState;
}

let motionFrame = 0;
let motionTracks: MotionTrack[] = [];

/** 停掉逐帧动画并还原内联样式；finished 时退场元素保持最终样式，等清理定时器移除 */
function cancelMotion(finished = false): void {
    if (motionFrame !== 0) {
        cancelAnimationFrame(motionFrame);
        motionFrame = 0;
    }
    for (const track of motionTracks) {
        if (!finished || track.keep !== true) {
            track.el.style.transform = '';
            track.el.style.opacity = '';
        }
    }
    motionTracks = [];
}

/**
 * 用 JS 逐帧驱动单词级动画：每个词一条轨道，位置与透明度都是时间的函数。
 * 第 0 帧同步画出（在浏览器绘制之前），避免元素先以最终状态闪一帧。
 * onFrame 在每帧画完后调用（选区底色据此跟随）。
 */
function playMotion(tracks: MotionTrack[], total: number, onFrame?: () => void): void {
    cancelMotion();
    motionTracks = tracks;
    const paint = (t: number): void => {
        for (const track of tracks) {
            const state = track.at(t);
            track.last = state;
            track.el.style.transform = 'translate(' + state.x + 'px, ' + state.y + 'px)';
            if (state.opacity != null) {
                track.el.style.opacity = String(state.opacity);
            }
        }
        onFrame?.();
    };
    paint(0);
    const start = performance.now();
    const step = (now: number): void => {
        const t = Math.min(total, now - start);
        paint(t);
        if (t < total) {
            motionFrame = requestAnimationFrame(step);
        } else {
            cancelMotion(true);
        }
    };
    motionFrame = requestAnimationFrame(step);
}

/**
 * 并入 / 撤销一侧上下文的整套逐词动画，四种情形共用（见 sentenceFlow.ts）：
 *
 * - 并入下文：下文那句（移动方）行内重排后上拼到正文末尾；
 * - 并入上文：原来的正文（移动方）行内重排后上拼到上文末尾，上文（宿主）文字原地变深，
 *   更早的一句从顶部整体滑入；
 * - 两者的撤销是各自的倒放。
 */
interface FlowLayer {
    id: 'm' | 'h';
    plan: FlowPlan;
}

interface FlowMotion {
    /** 时间方向：append = 并入（独立 → 合并），undo = 撤销（合并 → 独立） */
    direction: FlowDirection;
    layers: FlowLayer[];
    timing: FlowTiming;
    /** 后续下文（紧跟这句之后的其余句子）的纵向锚点 */
    trailing: TrailingAnchors;
    /** 移动方在独立布局里的全部片段 key：它们不算“后续下文” */
    blockKeys: Set<string>;
    /** 只在合并布局里存在的上文（并入上文时新进来的那句 / 撤销时离开的那句）：整体沿纵向进出 */
    edgeBlock: { fragments: DisplayFragment[]; piece: FlowPiece } | null;
    /** 动画开始前的片段与选区（离场的旧词要按旧选区决定是否带底色） */
    oldFragments: DisplayFragment[];
    oldSelection: PanelSelection | null;
}

function visibleChars(text: string): number {
    return text.replace(/\s+/gu, '').length;
}

function totalChars(fragments: DisplayFragment[]): number {
    return fragments.reduce((sum, fragment) => sum + visibleChars(fragment.text), 0);
}

/** 把合并后的正文从末尾切下 tailChars 个可见字符：前面是宿主（原样保留的开头），后面是移动方 */
function splitTail(
    fragments: DisplayFragment[],
    tailChars: number,
): { head: DisplayFragment[]; tail: DisplayFragment[] } | null {
    let chars = 0;
    let cut = fragments.length;
    while (cut > 0 && chars < tailChars) {
        cut -= 1;
        chars += visibleChars(fragments[cut].text);
    }
    if (chars !== tailChars) {
        return null;
    }
    return { head: fragments.slice(0, cut), tail: fragments.slice(cut) };
}

/**
 * 规划用的词盒：单词的盒子按粗体宽度预留、字形在盒内居中，所以用字形的真实位置做对齐，
 * 起点才能和旧文字逐字吻合（否则每个词会差半个预留量，开场就抖一下）。
 */
function glyphBox(fragment: DisplayFragment): WordBox {
    let x = fragment.x;
    let width = fragment.width;
    if (fragment.word) {
        const regular = domMeasure(fragment.text, measuredFont?.font ?? '');
        if (regular > 0 && regular < width) {
            x += (width - regular) / 2;
            width = regular;
        }
    }
    return { key: fragment.key, text: fragment.text, x, y: fragment.y, width };
}

/**
 * 规划一次并入 / 撤销。合并布局是新旧中带着并入内容的那份：并入时是新排版，撤销时是旧排版。
 * 两边文字对不上（比如接缝处的词元被改写）时返回 null，由调用方退回通用动画。
 */
function planFlowMotion(
    side: 'before' | 'after',
    direction: FlowDirection,
    oldFragments: DisplayFragment[],
    oldRegions: StackRegion[],
    oldSelection: PanelSelection | null,
): FlowMotion | null {
    const width = textEl.value?.clientWidth ?? 0;
    if (width <= 0 || sentenceLineHeight <= 0) {
        return null;
    }
    const merge = direction === 'append';
    const mergedFragments = merge ? laid.value : oldFragments;
    const standaloneFragments = merge ? oldFragments : laid.value;
    const mergedRegions = merge ? regions.value : oldRegions;
    const standaloneRegions = merge ? oldRegions : regions.value;
    const standaloneAfter = standaloneRegions.filter(region => region.role === 'after');
    const mergedAfter = mergedRegions.filter(region => region.role === 'after');
    const mergedSentence = mergedFragments.filter(fragment => fragment.role === 'sentence');
    const standaloneSentence = standaloneFragments.filter(fragment => fragment.role === 'sentence');
    const measure = (text: string): number => domMeasure(text, measuredFont?.font ?? '');
    const plan = (
        mergedPart: DisplayFragment[],
        standalonePart: DisplayFragment[],
        standaloneOpacity: number,
    ): FlowPlan | null => planFlow(
        mergedPart.map(glyphBox),
        standalonePart.map(glyphBox),
        sentenceLineHeight,
        width,
        measure,
        { standalone: standaloneOpacity, merged: 1 },
    );

    const layers: FlowLayer[] = [];
    let trailing: TrailingAnchors;
    let blockKeys = new Set<string>();
    let edgeBlock: FlowMotion['edgeBlock'] = null;

    if (side === 'after') {
        // 移动方是下文第一句：它拼到正文末尾；正文本身原地不动
        const block = standaloneAfter[0];
        const sentence = mergedRegions.find(region => region.role === 'sentence');
        if (block == null || sentence == null) {
            return null;
        }
        const mover = standaloneFragments.filter(fragment => fragment.role === 'after'
            && fragment.y >= block.top - 0.5 && fragment.y < block.bottom - 0.5);
        const split = splitTail(mergedSentence, totalChars(mover));
        if (split == null || totalChars(split.head) !== totalChars(standaloneSentence)) {
            return null;
        }
        const moverPlan = plan(split.tail, mover, CONTEXT_OPACITY);
        // 正文（宿主）并入前后也不一定原地不动：它的末行从“自然间距”变成中间行要被拉伸，
        // 个别情况下前面的行还会重新断行
        const hostPlan = plan(split.head, standaloneSentence, 1);
        if (moverPlan == null || hostPlan == null) {
            return null;
        }
        layers.push({ id: 'm', plan: moverPlan }, { id: 'h', plan: hostPlan });
        blockKeys = new Set(mover.map(fragment => fragment.key));
        trailing = trailingAnchors(
            standaloneAfter[1]?.top ?? block.bottom + CONTEXT_GAP,
            mergedAfter[0]?.top ?? sentence.bottom + CONTEXT_GAP,
            moverPlan.drop,
        );
    } else {
        // 移动方是原来的正文：它拼到上文末尾；上文是宿主，文字原地由淡变深
        const host = standaloneFragments.filter(fragment => fragment.role === 'before');
        if (host.length === 0) {
            return null;
        }
        const split = splitTail(mergedSentence, totalChars(standaloneSentence));
        if (split == null || totalChars(split.head) !== totalChars(host)) {
            return null;
        }
        const moverPlan = plan(split.tail, standaloneSentence, 1);
        const hostPlan = plan(split.head, host, CONTEXT_OPACITY);
        if (moverPlan == null || hostPlan == null) {
            return null;
        }
        layers.push({ id: 'm', plan: moverPlan }, { id: 'h', plan: hostPlan });
        // 合并布局顶部多出来的那句更早的上文：随宿主一起沿纵向整体进出，不渐入渐出
        const mergedBefore = mergedFragments.filter(fragment => fragment.role === 'before');
        if (mergedBefore.length > 0) {
            edgeBlock = {
                fragments: mergedBefore,
                piece: {
                    side: 'merged',
                    kind: 'stay',
                    dx: 0,
                    drop: hostPlan.drop,
                    line: 0,
                    standaloneOpacity: CONTEXT_OPACITY,
                    mergedOpacity: CONTEXT_OPACITY,
                },
            };
        }
        trailing = trailingAnchors(
            standaloneAfter[0]?.top ?? 0,
            mergedAfter[0]?.top ?? 0,
            moverPlan.drop,
        );
    }
    return {
        direction,
        layers,
        timing: flowTiming(Math.max(...layers.map(layer => layer.plan.maxShift))),
        trailing,
        blockKeys,
        edgeBlock,
        oldFragments,
        oldSelection,
    };
}

/** 需要留在屏幕上“离场”的旧片段：并入时是向右滑出的行末词，撤销时是从左滑走的行首词 */
function flowGhosts(motion: FlowMotion, newKeys: Set<string>): DisplayFragment[] {
    const merge = motion.direction === 'append';
    const edges = new Set<string>();
    for (const layer of motion.layers) {
        for (const [key, piece] of merge ? layer.plan.standalone : layer.plan.merged) {
            if (piece.kind === 'edge') {
                edges.add(key);
            }
        }
    }
    const leavingBlock = new Set(
        merge ? [] : motion.edgeBlock?.fragments.map(fragment => fragment.key) ?? [],
    );
    return motion.oldFragments
        .filter(fragment => edges.has(fragment.key)
            || leavingBlock.has(fragment.key)
            // 并入 / 撤销之外还消失的后续下文（比如撤销时被挤出去的第三句）
            || (fragment.role === 'after' && !newKeys.has(fragment.key) && !motion.blockKeys.has(fragment.key)))
        .map(fragment => ({
            ...fragment,
            leave: true,
            selected: fragment.role === 'sentence' && fragment.word
                && contains(motion.oldSelection, fragment.index),
        }));
}

/** 选区底色的一个成员：一个被选中的词，加上它在动画里的轨道 */
interface RectMember {
    x: number;
    y: number;
    width: number;
    height: number;
    track?: MotionTrack;
    group: string;
}

/**
 * 逐帧算出选区底色：被选中的词每帧在哪，底色就是它们的包围盒。
 *
 * 按“组”分底色——同一组的词在同一条带子上：层 + 它们在自己那份布局里的行号。
 * 并入时行末被挤走的词（离场的旧副本）与它那一行留下的词同组，所以起点的底色和并入前
 * 完全一致；终点则是并入后的整行底色。词怎么移，底色就怎么长，不会和文字脱节。
 */
function createRectDriver(members: RectMember[]): () => HighlightRect[] {
    const keys = new Map<string, string>();
    let primed = false;
    return () => {
        const groups = new Map<string, Box>();
        for (const member of members) {
            const state = member.track?.last;
            if (state != null && (state.opacity ?? 1) < 0.05) {
                continue;
            }
            const left = member.x + (state?.x ?? 0);
            const top = member.y + (state?.y ?? 0);
            const box = groups.get(member.group);
            if (box == null) {
                groups.set(member.group, {
                    left,
                    top,
                    right: left + member.width,
                    bottom: top + member.height,
                });
            } else {
                box.left = Math.min(box.left, left);
                box.top = Math.min(box.top, top);
                box.right = Math.max(box.right, left + member.width);
                box.bottom = Math.max(box.bottom, top + member.height);
            }
        }
        const list = Array.from(groups, ([group, box]) => ({ group, box }))
            .sort((a, b) => a.box.top - b.box.top || a.box.left - b.box.left);
        if (!primed) {
            // 第 0 帧沿用旧底色的 key：这样 Vue 认作同一块，不会先淡出再淡入
            primed = true;
            list.forEach((item, index) => {
                keys.set(item.group, rects.value[index]?.key ?? 'rect-' + keySeq++);
            });
        }
        return list.map(({ group, box }) => {
            let key = keys.get(group);
            if (key == null) {
                key = 'rect-' + keySeq++;
                keys.set(group, key);
            }
            return {
                key,
                left: box.left,
                top: box.top,
                width: box.right - box.left,
                height: box.bottom - box.top,
            };
        });
    };
}

function runFlowMotion(motion: FlowMotion): void {
    const host = textEl.value;
    if (host == null) {
        return;
    }
    const { direction, layers, timing, trailing } = motion;
    const merge = direction === 'append';
    // 真元素与离场副本的 data-key 相同（并入上文时正文的词既是真元素，也有一份离场副本），要分开找
    const realEls = new Map<string, HTMLElement>();
    const ghostEls = new Map<string, HTMLElement>();
    for (const el of Array.from(host.querySelectorAll<HTMLElement>('.token[data-key]'))) {
        (el.classList.contains('leaving') ? ghostEls : realEls).set(el.dataset.key ?? '', el);
    }
    // 同一帧里每个词都要同一份进度量，算一次即可
    let lastT = -1;
    let lastPhase = flowPhase(direction, 0, timing);
    const phaseAt = (t: number): ReturnType<typeof flowPhase> => {
        if (t !== lastT) {
            lastT = t;
            lastPhase = flowPhase(direction, t, timing);
        }
        return lastPhase;
    };
    const pieceTrack = (el: HTMLElement, piece: FlowPiece, ghost: boolean): MotionTrack => ({
        el,
        keep: ghost,
        at: t => {
            const phase = phaseAt(t);
            const offset = pieceOffset(piece, phase);
            return { x: offset.x, y: offset.y, opacity: pieceOpacity(piece, phase) };
        },
    });

    const tracks: MotionTrack[] = [];
    const realTracks = new Map<string, { track: MotionTrack; group: string }>();
    const ghostTracks = new Map<string, { track: MotionTrack; group: string }>();
    for (const layer of layers) {
        const real = merge ? layer.plan.merged : layer.plan.standalone;
        const ghost = merge ? layer.plan.standalone : layer.plan.merged;
        for (const [key, piece] of real) {
            const el = realEls.get(key);
            if (el != null) {
                const track = pieceTrack(el, piece, false);
                tracks.push(track);
                realTracks.set(key, { track, group: layer.id + ':' + piece.line });
            }
        }
        for (const [key, piece] of ghost) {
            const el = piece.kind === 'edge' ? ghostEls.get(key) : undefined;
            if (el != null) {
                const track = pieceTrack(el, piece, true);
                tracks.push(track);
                ghostTracks.set(key, { track, group: layer.id + ':' + piece.line });
            }
        }
    }
    // 合并布局里独有的那句上文：和宿主同一条纵向路径整体进出
    if (motion.edgeBlock != null) {
        for (const fragment of motion.edgeBlock.fragments) {
            const el = (merge ? realEls : ghostEls).get(fragment.key);
            if (el != null) {
                tracks.push(pieceTrack(el, motion.edgeBlock.piece, !merge));
            }
        }
    }

    // 后续下文：整体沿同一条纵向路径让位 / 归位。新出现的（并入下文时的第三句）淡入，
    // 被挤掉的（撤销时的第三句）淡出
    const planned = new Set<string>();
    for (const layer of layers) {
        for (const key of [...layer.plan.merged.keys(), ...layer.plan.standalone.keys()]) {
            planned.add(key);
        }
    }
    for (const fragment of motion.edgeBlock?.fragments ?? []) {
        planned.add(fragment.key);
    }
    const trailingEls = Array.from(host.querySelectorAll<HTMLElement>('.token.context[data-role="after"]'))
        .filter(el => {
            const key = el.dataset.key ?? '';
            return !planned.has(key) && !motion.blockKeys.has(key);
        });
    for (const el of trailingEls) {
        const ghost = el.classList.contains('leaving');
        const entering = el.classList.contains('entering');
        // 元素停在哪份布局的坐标上：真元素是新排版，幽灵是旧排版
        const ownMerged = merge !== ghost;
        const anchor = ownMerged ? trailing.merged : trailing.standalone;
        tracks.push({
            el,
            keep: ghost,
            at: t => {
                const state: MotionState = {
                    x: 0,
                    y: trailingY(trailing, phaseAt(t)) - anchor,
                };
                if (ghost || entering) {
                    const settled = progress(t, 0, timing.total);
                    state.opacity = CONTEXT_OPACITY * (ghost ? 1 - settled : settled);
                }
                return state;
            },
        });
    }

    // 选区底色：被选中的词（真元素加离场副本）每帧在哪，底色就跟到哪
    const members: RectMember[] = [];
    const selection = props.selection;
    if (selection != null) {
        for (const fragment of laid.value) {
            if (fragment.role !== 'sentence' || fragment.index < selection.start
                || fragment.index > selection.end || fragment.width < 0.5) {
                continue;
            }
            const entry = realTracks.get(fragment.key);
            members.push({
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
                track: entry?.track,
                group: entry?.group ?? 'static:' + Math.round(fragment.y),
            });
        }
    }
    const old = motion.oldSelection;
    if (old != null) {
        for (const fragment of motion.oldFragments) {
            const entry = ghostTracks.get(fragment.key);
            if (entry == null || fragment.role !== 'sentence' || fragment.index < old.start
                || fragment.index > old.end || fragment.width < 0.5) {
                continue;
            }
            members.push({
                x: fragment.x,
                y: fragment.y,
                width: fragment.width,
                height: fragment.height,
                track: entry.track,
                group: entry.group,
            });
        }
    }
    const driver = createRectDriver(members);
    playMotion(tracks, timing.total, () => {
        rects.value = driver();
    });
}

/**
 * 非“并入下文”的句子变化（并入上文、撤销、恢复）的逐词动画：
 * 全部用 WAAPI 做 FLIP——先量新旧位置差，再把词从旧位置补间到最终位置。
 */
function runReflowAnimation(
    oldFragments: DisplayFragment[],
    oldRegions: StackRegion[],
    duration: number,
): void {
    const host = textEl.value;
    if (host == null || reducedMotion()) {
        return;
    }
    const oldByKey = new Map(oldFragments.map(fragment => [fragment.key, fragment]));
    for (const fragment of laid.value) {
        const el = host.querySelector<HTMLElement>('[data-key="' + fragment.key + '"]');
        if (el == null) {
            continue;
        }
        const old = oldByKey.get(fragment.key);
        let animation: Animation;
        if (old != null) {
            const dx = old.x - fragment.x;
            const dy = old.y - fragment.y;
            if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) {
                continue;
            }
            animation = el.animate(
                [
                    { transform: 'translate(' + dx + 'px, ' + dy + 'px)' },
                    { transform: 'translate(0, 0)' },
                ],
                { duration, easing: 'cubic-bezier(0.22, 0.61, 0.36, 1)', fill: 'both' },
            );
        } else {
            // 上下文片段的静态不透明度是 45%，补间要落到它自己的基准值上，
            // 否则新句会先亮到 100% 再“熄”回去
            const resting = Number.parseFloat(getComputedStyle(el).opacity);
            animation = el.animate(
                [
                    { opacity: 0, transform: 'translateY(' + shift + ')' },
                    { opacity: Number.isFinite(resting) ? resting : 1, transform: 'translateY(0)' },
                ],
                {
                    duration,
                    easing: 'cubic-bezier(0.22, 0.61, 0.36, 1)',
                    fill: 'both',
                    delay: Number.parseFloat(enterDelay.value) || 0,
                },
            );
        }
        appendAnimations.push(animation);
        animation.onfinish = () => {
            animation.cancel();
            appendAnimations = appendAnimations.filter(item => item !== animation);
        };
    }
    // 上下文区域（渐变层与点击区）：跟着最近的片段一起挪，避免渐变线跳位
    for (const region of contextRegions.value) {
        const el = host.querySelector<HTMLElement>('.context-hit.' + region.role);
        // 同侧只圈第一句，所以取第一块的范围，与 contextRegions 一一对应
        const old = oldRegions.find(item => item.role === region.role);
        if (el == null || old == null) {
            continue;
        }
        const dy = old.top - region.top;
        if (Math.abs(dy) < 0.5) {
            continue;
        }
        const animation = el.animate(
            [
                { transform: 'translateY(' + dy + 'px)' },
                { transform: 'translateY(0)' },
            ],
            { duration, easing: 'cubic-bezier(0.22, 0.61, 0.36, 1)', fill: 'both' },
        );
        appendAnimations.push(animation);
        animation.onfinish = () => {
            animation.cancel();
            appendAnimations = appendAnimations.filter(item => item !== animation);
        };
    }
    // 被删掉的词：留在原位淡出（ghost 元素由模板渲染）
    for (const fragment of exiting.value) {
        const el = host.querySelector<HTMLElement>('.token.leaving[data-key="' + fragment.key + '"]');
        if (el == null) {
            continue;
        }
        const animation = el.animate(
            [
                { opacity: 1, transform: 'translate(0, 0)' },
                { opacity: 0, transform: 'translateY(' + shift + ')' },
            ],
            { duration: 180, easing: 'cubic-bezier(0.4, 0, 1, 1)', fill: 'both' },
        );
        appendAnimations.push(animation);
        animation.onfinish = () => {
            animation.cancel();
            appendAnimations = appendAnimations.filter(item => item !== animation);
        };
    }
}


watch(() => props.tokens, () => {
    const texts = props.tokens.map(token => token.token);
    const previous = previousTokenTexts;
    const change = classifySentenceChange(previous.join(''), texts.join(''));
    tokenKeys = remapTokenKeys(previous, tokenKeys, texts, change, () => keySeq++);
    previousTokenTexts = texts;

    if (!laidOnce) {
        // 首次上屏：直接排版，不播动画
        laidOnce = relayout();
        return;
    }
    if (change.kind === 'replace' || reducedMotion()) {
        // 整句替换（重新取词、粘贴、编辑）没有可延续的位移，直接换；
        // 系统要求减弱动效时同样直接到位
        cancelMotion();
        flowRects.value = false;
        exiting.value = [];
        relayout();
        sentenceMoving.value = false;
        return;
    }

    // 新的动作立刻接管：上一轮还没播完的入场动画先取消
    appendAnimations.forEach(animation => animation.cancel());
    appendAnimations = [];
    cancelMotion();
    flowRects.value = false;

    const oldFragments = laid.value;
    const oldRegions = regions.value;
    const oldKeys = new Set(oldFragments.map(fragment => fragment.key));
    // 抵消高度过渡期间正文块还没长到最终高度的那段差值
    shift = change.kind === 'append' || change.kind === 'shrink-end' ? '8px' : '-8px';
    // 并入上文：旧块先下移让出位置，新块再浮现；撤销并入上文：被删的块先淡走
    // 并入上文时旧词正在整体移动，新词稍晚一点浮现，避免中途叠字
    enterDelay.value = change.kind === 'prepend' ? '120ms' : '0ms';
    relayout();
    const newKeys = new Set(laid.value.map(fragment => fragment.key));
    laid.value = laid.value.map(fragment => ({ ...fragment, enter: !oldKeys.has(fragment.key) }));

    // 并入 / 撤销一侧上下文：同一套逐词动画；规划不出来（文字对不上）就退回通用动画
    const mergeSide = change.kind === 'append' || change.kind === 'prepend';
    const flowSide = change.kind === 'append' || change.kind === 'shrink-end'
        ? 'after'
        : change.kind === 'prepend' || change.kind === 'shrink-start'
            ? 'before'
            : null;
    const flow = flowSide == null
        ? null
        : planFlowMotion(flowSide, mergeSide ? 'append' : 'undo', oldFragments, oldRegions, shown);
    // 只有正文会“退场”淡出：上下文的换挡是整块位移，由各自的时间线负责
    exiting.value = flow != null
        ? flowGhosts(flow, newKeys)
        : oldFragments
            .filter(fragment => fragment.role === 'sentence' && !newKeys.has(fragment.key))
            .map(fragment => ({ ...fragment, leave: true }));
    flowRects.value = flow != null;

    let motionMs = SENTENCE_MOTION_MS;
    if (flow != null) {
        motionMs = flow.timing.total;
        void nextTick(() => runFlowMotion(flow));
    } else {
        void nextTick(() => runReflowAnimation(oldFragments, oldRegions, SENTENCE_MOTION_MS));
    }
    sentenceMoving.value = true;
    if (exitTimer != null) {
        clearTimeout(exitTimer);
    }
    exitTimer = setTimeout(() => {
        exiting.value = [];
        laid.value = laid.value.map(fragment => ({ ...fragment, enter: false }));
        appendAnimations.forEach(animation => animation.cancel());
        appendAnimations = [];
        cancelMotion();
        flowRects.value = false;
        sentenceMoving.value = false;
        exitTimer = null;
        void updateRects();
    }, motionMs + 40);
}, { immediate: true });

function beginPress(event: MouseEvent): void {
    pressed.value = true;
    dragStarted.value = false;
    pressX = event.clientX;
    pressY = event.clientY;
    pressMoved = false;
    window.addEventListener('mousemove', onWindowMouseMove);
    window.addEventListener('mouseup', onWindowMouseUp);
    window.addEventListener('blur', onWindowBlur);
}

function removeWindowListeners(): void {
    window.removeEventListener('mousemove', onWindowMouseMove);
    window.removeEventListener('mouseup', onWindowMouseUp);
    window.removeEventListener('blur', onWindowBlur);
}

/**
 * 结束手势。commit 区分收尾原因：用户松开或窗口失焦属于手势完成，应上行 commit
 * 让父组件快送查询；分词被整体替换时选区不是用户写的，只收尾不上行
 */
function endGesture(commit: boolean): void {
    if (!pressed.value) {
        return;
    }
    const wasDragging = dragStarted.value;
    pressed.value = false;
    dragStarted.value = false;
    removeWindowListeners();
    // 按下时停在“已选中的单个词”上：只有“干净的一下”（没拖动、位移没超过阈值）才算取消
    if (pendingToggle != null) {
        const index = pendingToggle;
        pendingToggle = null;
        if (commit && !wasDragging && !pressMoved && isSame(emitted, { start: index, end: index })) {
            emitSelection(null);
        }
    }
    if (commit) {
        emit('commit');
    }
}

/**
 * 按下：先确定这次按下的语义，再进入可拖选的状态
 * （按下即生效，与文本控件一致；拖动只是把焦点端拉远）
 */
function onMouseDown(event: MouseEvent): void {
    // 右键、中键、Ctrl/⌘+点击（macOS 的右键约定）不参与选中
    if (event.button !== 0 || event.ctrlKey || event.metaKey) {
        return;
    }
    const panel = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
    if (panel != null) {
        const rect = panel.getBoundingClientRect();
        // 点在滚动条上：交给原生滚动。下面的 preventDefault 会把滚动条拖拽一起挡掉
        if (event.clientX - rect.left >= panel.clientWidth || event.clientY - rect.top >= panel.clientHeight) {
            return;
        }
    }
    // 阻止原生文本选择与拖拽：面板里的选中全部由本组件接管
    event.preventDefault();
    if (pressed.value) {
        endGesture(false);
    }
    const index = indexAt(event);
    if (index == null || !isWordIndex(index)) {
        // 点在标点、空白或面板空处：什么也不做。
        // 句子里总要有一个被标记的词（应用就是“结合语境理解单词”），清空不是有用的动作，
        // 却最容易误触——而且清空后加卡，例句会连加粗词一起丢掉。
        // 取消仍然可达，只是入口落在对象自己身上：再点一次已选中的词
        return;
    }
    beginPress(event);
    // 用最近一次上报的选区而不是 props：props 要等父组件回传，同一 tick 内的连续手势会读到旧值
    const current = emitted;
    if (event.shiftKey && current != null) {
        // Shift+按下：固定端不动，把焦点端拉到这个词（文本选区扩展 / 收缩的既有习惯）
        anchor = anchor ?? current.start;
        emitSelection(normalize(anchor, index));
        return;
    }
    anchor = index;
    if (contains(current, index)) {
        // 点在选区内：收窄到这个词；只有“选区本来就是这个词”时才是取消，
        // 等价于文本控件里点选区内塌陷到落点
        if (current != null && current.start === current.end) {
            pendingToggle = index;
        } else {
            emitSelection({ start: index, end: index });
        }
        return;
    }
    emitSelection({ start: index, end: index });
}

/** 拖选：焦点端跟着指针走，只认单词词元；指到标点或面板外时焦点停在最近的单词上 */
function onWindowMouseMove(event: MouseEvent): void {
    if (!pressed.value || anchor == null) {
        return;
    }
    if (!pressMoved
        && (Math.abs(event.clientX - pressX) > CLICK_SLOP_PX || Math.abs(event.clientY - pressY) > CLICK_SLOP_PX)) {
        pressMoved = true;
    }
    const index = indexAt(event);
    if (index == null || !isWordIndex(index)) {
        return;
    }
    if (index !== anchor) {
        dragStarted.value = true;
        pendingToggle = null;
    }
    emitSelection(normalize(anchor, index));
}

function onWindowMouseUp(): void {
    endGesture(true);
}

function onWindowBlur(): void {
    // 切走窗口不是“点了一下”：先放弃延迟取消，免得按在选中词上切个窗口就把它取消了
    pendingToggle = null;
    // 拖选中切走窗口导致 mouseup 丢失：选区已随手势落下，按手势完成快送一次查询
    endGesture(true);
}

function onPanelMouseMove(event: MouseEvent): void {
    hoverIndex.value = pressed.value ? null : indexAt(event);
}

function onPanelMouseLeave(): void {
    hoverIndex.value = null;
}

/**
 * 父组件整体替换选区（取词预选、编辑确认重建）：锚点跟随新区间的起点，
 * 这样再一次 Shift+按下是从可见选区出发扩展，而不是从上一句话的旧锚点
 */
watch(() => props.selection, value => {
    if (!isSame(value, emitted)) {
        emitted = value;
        anchor = value != null ? value.start : null;
    }
    void updateRects();
});

/**
 * 父组件整体替换分词（划词录入、粘贴、编辑确认）：旧下标与手势锚点全部失效，
 * 收尾进行中的手势但不上行 commit——这次变化不是用户手势的产物
 */
watch(() => props.tokens, () => {
    endGesture(false);
    pendingToggle = null;
    anchor = props.selection != null ? props.selection.start : null;
    void updateRects();
});

let lastWidth = 0;
/** 面板宽度变化才重排：高度变化是文本自己在长，重排会打断补间 */
const resizeObserver = new ResizeObserver(() => {
    const width = textEl.value?.clientWidth ?? 0;
    if (width === lastWidth) {
        return;
    }
    lastWidth = width;
    resizing.value = true;
    relayout();
    void updateRects().then(() => {
        requestAnimationFrame(() => {
            resizing.value = false;
        });
    });
});

onMounted(() => {
    emitted = props.selection;
    anchor = props.selection != null ? props.selection.start : null;
    lastWidth = textEl.value?.clientWidth ?? 0;
    if (bodyEl.value != null) {
        resizeObserver.observe(bodyEl.value);
    }
    if (!laidOnce) {
        laidOnce = relayout();
    }
    void updateRects();
});

onBeforeUnmount(() => {
    resizeObserver.disconnect();
    removeWindowListeners();
    if (exitTimer != null) {
        clearTimeout(exitTimer);
    }
    appendAnimations.forEach(animation => animation.cancel());
    cancelMotion();
});
</script>

<template>
    <div class="sentence-panel" :class="{ pressing: pressed }" @mousedown="onMouseDown"
        @mousemove="onPanelMouseMove" @mouseleave="onPanelMouseLeave">
        <div ref="bodyEl" class="sentence-body">
            <div ref="textEl" class="sentence-text" :class="{ flowing: flowRects }" :style="{ height: textHeight + 'px' }">
                <TransitionGroup tag="div" class="selection-layer" name="selection-rect" :css="!flowRects" aria-hidden="true">
                    <div v-for="rect in rects" :key="rect.key" class="selection-rect" :class="{
                        'no-anim': resizing || flowRects,
                        dragging: dragStarted && !resizing,
                        swapping: moving && !dragStarted && !resizing,
                        'sentence-move': sentenceMoving,
                    }" :style="{
                        left: rect.left + 'px',
                        top: rect.top + 'px',
                        width: rect.width + 'px',
                        height: rect.height + 'px',
                    }" />
                </TransitionGroup>
                <span v-for="fragment in laid" :key="fragment.key" class="token" :class="{
                    'is-word': fragment.word && fragment.role === 'sentence',
                    'not-word': !fragment.word || fragment.role !== 'sentence',
                    context: fragment.role !== 'sentence',
                    hovered: fragment.role === 'sentence' && hoveredIndex === fragment.index,
                    selected: fragment.role === 'sentence' && fragment.word
                        && contains(props.selection, fragment.index),
                    entering: fragment.enter,
                }" :data-token="fragment.role === 'sentence' ? fragment.index : undefined"
                    :data-role="fragment.role" :data-key="fragment.key" :style="{
                    left: fragment.x + 'px',
                    top: fragment.y + 'px',
                    width: fragment.width + 'px',
                    height: fragment.height + 'px',
                    lineHeight: fragment.height + 'px',
                }">{{ fragment.text }}</span>
                <span v-for="fragment in exiting" :key="'exit-' + fragment.key" class="token leaving" :class="{
                    selected: fragment.selected === true,
                    'is-word': fragment.word && fragment.role === 'sentence',
                    'not-word': !fragment.word || fragment.role !== 'sentence',
                    context: fragment.role !== 'sentence',
                }" :data-key="fragment.key" :data-role="fragment.role"
                    :data-token="fragment.role === 'sentence' ? fragment.index : undefined" :style="{
                    left: fragment.x + 'px',
                    top: fragment.y + 'px',
                    width: fragment.width + 'px',
                    height: fragment.height + 'px',
                    lineHeight: fragment.height + 'px',
                }">{{ fragment.text }}</span>
                <!-- 上下文区域：点击并入该侧最近的一句；渐隐与悬停底色都画在这一层上，
                     文字本身仍是排版片段，跟正文同一套坐标 -->
                <div v-for="region in contextRegions" :key="'region-' + region.role" class="context-hit"
                    :class="[region.role, { moving: sentenceMoving }]" :style="{
                        top: region.top + 'px',
                        height: (region.bottom - region.top) + 'px',
                    }" @mousedown.stop @click.stop="onContextClick(region.role)"></div>
            </div>
        </div>
    </div>
</template>

<style scoped>
.sentence-panel {
    /* 选区底色：**独立的高亮色**，不是把强调色压成半透明。
       强调色 #0067c0 压到 30% 落在近白底上会合成出 #B0CFE9 这类低彩度灰蓝（饱和度只剩约
       56%，系统选区约 100%），看着发脏、也不够醒目。这里直接用不透明的高彩度浅蓝，
       对齐系统文本选区的观感；文字颜色保持原样，几何补间因此不受影响 */
    --selection-fill: #b3d7ff;
    /* 悬停底色只比选区弱一档：提示这个词可以点，又不至于看起来像已选中 */
    --token-hover-fill: color-mix(in srgb, var(--accent) 9%, transparent);
    /* 动效按“动作幅度”分三档，全面板只用这三个数值：
       follow = 跟手（拖选、光标跟随）
       fade   = 微交互与显隐（悬停提亮、选区出现/消失、新文字落定）
       move   = 空间移动（选区挪位、整块滑动、裁剪揭示）
       距离越短用越短的档位；不再为单个控件发明新数值 */
    --motion-follow: 90ms;
    --motion-fade: 160ms;
    --motion-move: 260ms;
    /* 句子级重排：位移、底色、文本块高度共用同一条时长与曲线 */
    --sentence-duration: 320ms;
    --sentence-ease: cubic-bezier(0.45, 0.05, 0.2, 1);
    --selection-resize-duration: var(--motion-fade);
    --selection-follow-duration: var(--motion-follow);
    --selection-move-duration: var(--motion-move);
    --selection-enter-duration: var(--motion-fade);
    --selection-exit-duration: var(--motion-fade);

    width: 100%;
    height: 100%;
    padding: 8px 12px;
    user-select: none;
    /* 面板本身不是可点区域：空白处是箭头（点它什么也不会发生），只有单词词元才变成 I-beam */
    cursor: default;
    /* 与释义卡片同一底色（浅色纯白 / 深色 #1e1e1eb3）：阅读面干净，
       也不会用一层灰底把高亮色的彩度再拉低一档 */
    background-color: var(--input-text-background-focus);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
    overflow-y: auto;
    overflow-wrap: break-word;
}

/* 拖选中保持 I-beam：扫过标点与空白时光标不该来回跳 */
.sentence-panel.pressing {
    cursor: text;
}

/* 深色主题：正文是浅色字，浅蓝底会让它糊成一片，换成深蓝底；文字颜色仍然不变 */
html.dark .sentence-panel {
    --selection-fill: #264f78;
}

/* 上下文（前后各一到两句）：和正文是同一批排版片段，只靠浓淡区分。
   逐字的坐标由排版给出，所以“正文变高、下文跟着下移”只是 y 的变化，
   不存在另一个控件自己算一套位置 */
.token.context {
    opacity: 0.45;
    cursor: default;
}

/* 上下文区域：由单词坐标算出的包围盒，只是一层覆盖在文字上方的命中区——
   负责悬停底色与点击并入，不参与排版，也不裁切任何文字 */
.context-hit {
    position: absolute;
    left: 0;
    right: 0;
    z-index: 1;
    border-radius: 2px;
    cursor: default;
    transition: background-color var(--motion-fade) var(--motion-ease-standard);
}

/* 悬停：给出“可以点”的底色（与词悬停同款）。用 CSS :hover 而不是 JS 状态：
   节点被替换或窗口失焦时收不到 mouseleave，:hover 永远跟随真实指针 */
.context-hit:hover {
    background-color: var(--token-hover-fill);
}

/* 句子级动画期间：命中区不出现悬停底色、也不接收指针。文字正在逐词移动，
   一整块底色停在原地（或跟着挪）只会和文字错位；动画结束后恢复 */
.context-hit.moving,
.context-hit.moving:hover {
    background-color: transparent;
    pointer-events: none;
}

.sentence-text {
    position: relative;
    display: block;
    width: 100%;
    /* 只裁横向：换行词要从行首外滑进来，不裁会撑出横向滚动条 */
    overflow-x: clip;
    overflow-y: visible;
    /* 高度直接给最终值：块里全是绝对定位的片段，位置由补间负责，
       高度再补间只会让滚动范围慢半拍 */
}

/* 逐词动画期间：从顶部进来 / 离开的那句上文只在文字区内可见，不会把字底边露到面板边框里 */
.sentence-text.flowing {
    clip-path: inset(0 0 -100vh 0);
}

.sentence-body {
    position: relative;
    /* 建立层叠上下文：让下面 z-index:-1 的底色层只落到正文之下，
       而不是钻到面板背景后面（否则整块色块会被背景盖住） */
    isolation: isolate;
}

/* 选区底色层：绝对定位在文字**下方**（负 z-index），不参与布局也不拦指针。
   注意：只写 position: absolute 会让它画在行内文字之上——底色一旦不透明就会盖住文字 */
.selection-layer {
    position: absolute;
    inset: 0;
    z-index: -1;
    pointer-events: none;
}

.selection-rect {
    position: absolute;
    border-radius: 2px;
    background-color: var(--selection-fill);
    transition:
        left var(--selection-resize-duration) var(--motion-ease-standard),
        top var(--selection-resize-duration) var(--motion-ease-standard),
        width var(--selection-resize-duration) var(--motion-ease-standard),
        height var(--selection-resize-duration) var(--motion-ease-standard);
}

/* 拖选时底色跟着指针逐词走，补间要更短 */
.selection-rect.dragging {
    transition-duration: var(--selection-follow-duration);
}

/* 替换：同一块底色挪到新位置，位置补间更长，让“移动”本身可读 */
.selection-rect.swapping {
    transition-duration: var(--selection-move-duration);
}

/* 面板尺寸变化时文本会重新折行，位置直接到位 */
.selection-rect.no-anim {
    transition: none;
}

/* 句子级重排：底色与文字补间用完全相同的时长与曲线，两者才不会脱节 */
.selection-rect.sentence-move {
    transition-duration: var(--sentence-duration);
    transition-timing-function: var(--sentence-ease);
}

/* 从无到有：淡入 */
.selection-rect-enter-active {
    transition: opacity var(--selection-enter-duration) var(--motion-ease-standard);
}

.selection-rect-enter-from {
    opacity: 0;
}

/* 取消：原地淡出，不位移、不收边——边缘内缩已经属于“收窄到某个词”的语汇。
   opacity 走 linear：标准曲线起步太快，160ms 的淡出在第一帧就掉到三成不透明度，看着像闪断；
   叠 2% 的缩小只是让画面不至于像“一下子没了”，肉眼几乎看不出位移 */
.selection-rect-leave-active {
    transition:
        opacity var(--selection-exit-duration) linear,
        transform var(--selection-exit-duration) linear;
}

.selection-rect-leave-to {
    opacity: 0;
    transform: scale(0.98);
}

/* 测量专用容器：不参与布局、不可见，只用来读出文字的真实宽度。
   里面的 span 复用 .token 的字体，量到的就是浏览器渲染时会用的宽度 */
.measure-host {
    position: absolute;
    top: 0;
    left: 0;
    visibility: hidden;
    pointer-events: none;
}

.token {
    position: absolute;
    box-sizing: border-box;
    white-space: pre;
    text-align: center;
    font-size: var(--font-size);
    font-family: var(--font-family);
    border-radius: 2px;
    user-select: none;
    cursor: inherit;
    /* 位置由 JS 补间（WAAPI）：这里不能再写 left/top 的 CSS 过渡，
       否则同一段位移会被两套补间叠加，走成两倍距离 */
}

/* 退场的字：向来源方向浮走，再移除 */
.token.leaving {
    pointer-events: none;
}

/* 只有单词词元可选：I-beam 就是“点这里选这个词”的暗示 */
.token.is-word {
    cursor: text;
}

/* 不是单词的空白字符 */
.token.not-word {
    cursor: inherit;
}

/* 悬停只给一层最轻的底色：不画框——框会暗示它是独立控件，而这里的语义是“文本里的词” */
.token.is-word.hovered {
    background-color: var(--token-hover-fill);
}

/* 选中：只把字形层换成真粗体。盒子宽度由幽灵层固定，所以加粗不会引起任何重排 */
.token.is-word.selected {
    font-weight: bold;
}

@media (prefers-reduced-motion: reduce) {
    .selection-rect,
    .selection-rect.dragging,
    .selection-rect.swapping,
    .selection-rect-enter-active,
    .selection-rect-leave-active {
        transition: none;
    }
}
</style>
