<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type PropType } from 'vue';

import * as utils from '../logics/utils';

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

const props = defineProps({
    tokens: {
        type: Array as PropType<PanelToken[]>,
        required: true,
    },
    selection: {
        type: Object as PropType<PanelSelection | null>,
        default: null,
    },
});

const emit = defineEmits<{
    /** 选区变化（手势进行中逐次上报；写入方是父组件） */
    'update:selection': [value: PanelSelection | null];
    /** 手势结束（单击松手、拖选松手、Shift 区间松手）：父组件据此把选区快送成一次查询 */
    commit: [];
}>();

const bodyEl = ref<HTMLElement>();
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
/** 矩阵 key 的唯一来源（元素因此能被复用、位置才能补间） */
let keySeq = 0;
/** 上一轮显示的选区：判断这次是伸缩还是替换 */
let shown: PanelSelection | null = null;

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
 * 量出选区覆盖的底色矩形：把区间内每个词元的 border box 按行合并，
 * 行内的标点与空格因此被一并覆盖，一整段选区看起来是一块连续的底色
 */
function measureLines(): Box[] {
    const body = bodyEl.value;
    const selection = props.selection;
    if (body == null || selection == null) {
        return [];
    }
    const base = body.getBoundingClientRect();
    const tokenEls = body.querySelectorAll<HTMLElement>('[data-token]');
    const boxes: Box[] = [];
    const last = Math.min(selection.end, tokenEls.length - 1);
    for (let index = Math.max(selection.start, 0); index <= last; index++) {
        for (const rect of tokenEls[index].getClientRects()) {
            // 折行处的空白可能是零宽矩形，收进来只会把底色拖到行末
            if (rect.width < 0.5) {
                continue;
            }
            boxes.push({
                left: rect.left - base.left,
                top: rect.top - base.top,
                right: rect.right - base.left,
                bottom: rect.bottom - base.top,
            });
        }
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
 *
 * - 伸缩：按行顶对齐复用（同一行在选区变化前后 top 不变）；
 * - 替换：第一块沿用上一轮第一块的 key —— 于是它不是“消失再出现”，而是**挪过去**；
 *   跨行替换时多出来的行淡入、少掉的行淡出。
 */
function layoutRects(lines: Box[], mode: 'enter' | 'resize' | 'swap'): HighlightRect[] {
    const previous = rects.value;
    const used = new Set<string>();
    const flying = mode === 'swap' ? previous[0] : undefined;
    return lines.map((line, index) => {
        let key: string;
        if (index === 0 && flying != null) {
            key = flying.key;
        } else {
            const match = previous.find(r => !used.has(r.key) && Math.abs(r.top - line.top) < 1.5);
            key = match?.key ?? `rect-${keySeq++}`;
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
    const value = props.selection;
    if (value == null) {
        // 取消：矩形移出列表，交给 leave 过渡原地淡出（不位移、不收边）
        shown = null;
        rects.value = [];
        return;
    }
    const mode = shown == null ? 'enter' : (overlaps(shown, value) ? 'resize' : 'swap');
    shown = value;
    moving.value = mode === 'swap';
    rects.value = layoutRects(measureLines(), mode);
}

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

/** 面板尺寸变化会重新折行：位置直接到位，不做补间，免得底色滞后于文字 */
const resizeObserver = new ResizeObserver(() => {
    resizing.value = true;
    void updateRects().then(() => {
        requestAnimationFrame(() => {
            resizing.value = false;
        });
    });
});

onMounted(() => {
    emitted = props.selection;
    anchor = props.selection != null ? props.selection.start : null;
    if (bodyEl.value != null) {
        resizeObserver.observe(bodyEl.value);
    }
    void updateRects();
});

onBeforeUnmount(() => {
    resizeObserver.disconnect();
    removeWindowListeners();
});
</script>

<template>
    <div class="sentence-panel" :class="{ pressing: pressed }" @mousedown="onMouseDown"
        @mousemove="onPanelMouseMove" @mouseleave="onPanelMouseLeave">
        <div ref="bodyEl" class="sentence-body">
            <TransitionGroup tag="div" class="selection-layer" name="selection-rect" aria-hidden="true">
                <div v-for="rect in rects" :key="rect.key" class="selection-rect" :class="{
                    'no-anim': resizing,
                    dragging: dragStarted && !resizing,
                    swapping: moving && !dragStarted && !resizing,
                }" :style="{
                    left: `${rect.left}px`,
                    top: `${rect.top}px`,
                    width: `${rect.width}px`,
                    height: `${rect.height}px`,
                }" />
            </TransitionGroup>
            <span v-for="(token, index) in tokens" :key="index" class="token" :class="{
                'is-word': isWordIndex(index),
                'not-word': !isWordIndex(index),
                hovered: hoveredIndex === index,
                selected: isWordIndex(index) && contains(props.selection, index),
            }" :data-token="index"><template v-if="isWordIndex(index)"><span class="token-ghost"
                        aria-hidden="true">{{ token.token }}</span><span class="token-glyph">{{ token.token }}</span></template><template
                    v-else>{{ token.token }}</template></span>
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
    /* 点一下、Shift 扩展这类离散伸缩：短到不黏手，又足以看清边界在移动 */
    --selection-resize-duration: 160ms;
    /* 拖选跟手时用更短的补间：再长就明显落后于指针 */
    --selection-follow-duration: 90ms;
    /* 替换时整块挪过去：比伸缩长一点，让“移动”被看清，而不是像闪烁 */
    --selection-move-duration: 200ms;
    /* 出现：淡入。选区是直接操作的结果，手感贴近系统文本选区，比就地反馈的进场（180ms）短 */
    --selection-enter-duration: 120ms;
    /* 取消：原地淡出。比入场略长、起步更缓，才看得出“被收起”的过程；再短就退化成闪断 */
    --selection-exit-duration: 160ms;

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

.token {
    white-space: pre-wrap;
    overflow-wrap: break-word;
    font-size: var(--font-size);
    font-family: var(--font-family);
    /* 词元左右各留 2px：相邻词元（含标点）的盒子因此首尾相接，选区底色没有缝 */
    padding-left: 2px;
    padding-right: 2px;
    border-radius: 2px;
    user-select: none;
    /* 标点与空白跟随面板（箭头 / 拖选中 I-beam） */
    cursor: inherit;
}

/* 单词词元：盒子按“加粗后”的宽度预留，选中与否排版都不变。
   盒子里叠两层同样的文本：
   - 幽灵层：隐藏、始终粗体，只负责把宽度撑到加粗后的尺寸；
   - 字形层：可见，绝对定位铺满盒子并居中——常规字重时两侧各余一半差宽，
     不会出现“只有词尾突然多出一截”的空隙，也就不会把标点和词分开。
   词元的左右内边距归零：预留出来的那半份字宽本身就充当了字形两侧的余量，
   再叠 2px 内边距会把词距撑得过宽（实测 SF 下粗体宽 3.8px，正好抵掉原来的 2+2px） */
.token.is-word {
    display: inline-block;
    position: relative;
    text-align: center;
    padding-left: 0;
    padding-right: 0;
    /* 超长单词（URL 等）必须能在词内折行：inline-block 的拉伸宽度按 max-content 算，
       只写 break-word 会撑出横向滚动条；anywhere 才会参与收缩宽度的计算 */
    overflow-wrap: anywhere;
}

.token-ghost {
    visibility: hidden;
    font-weight: bold;
}

.token-glyph {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    text-align: center;
}

/* 只有单词词元可选：I-beam 就是“点这里选这个词”的暗示 */
.token.is-word {
    cursor: text;
}

/* 不是单词的空白字符 */
.token.not-word {
    padding-left: 0;
    padding-right: 0;
}

/* 悬停只给一层最轻的底色：不画框——框会暗示它是独立控件，而这里的语义是“文本里的词” */
.token.is-word.hovered {
    background-color: var(--token-hover-fill);
}

/* 选中：只把字形层换成真粗体。盒子宽度由幽灵层固定，所以加粗不会引起任何重排 */
.token.is-word.selected .token-glyph {
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
