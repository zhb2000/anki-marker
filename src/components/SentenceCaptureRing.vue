<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue';

/**
 * 句子面板角上的"仍在补取完整句子"指示环。
 *
 * 不是通用的进度控件（故不叫 FluentProgressRing）：它只服务划词取句这一处，且退场
 * 刻意偏离 WinUI ProgressRing 的规格——WinUI 在 IsActive=false 时调用 player.Stop()
 * 并把透明度置 0（当场停住），这里则让弧长收束到 0 再隐藏。
 *
 * 收束到 0 的收益是"消失"本身不可感知：隐藏发生在环上已无可见物的一刻，因此不需要
 * "最短可见时长"这类人为常量——最短可见即入场与退场两段动画自身的时长。
 * 循环期的弧长不取 0（否则工作期间环会周期性整个消失，反而像闪烁），只在退场收束时归零。
 *
 * 收束是**尾巴顺着头部的方向追上来**（头部沿路径不动、尾巴向前扫过），而不是反过来
 * 把头部往回收——后者在屏幕上是一段逆着旋转方向的倒放，观感像卡带倒转。
 *
 * 逐帧动画由 requestAnimationFrame 直接驱动而非 CSS keyframes：CSS 改
 * animation-name 会重启动画、相位跳变，无法从中途优雅收束。
 */

/** 是否处于"补取在途"：转为 false 时播放收束动画，弧长归零后自行隐藏 */
const props = defineProps<{ active: boolean }>();

/** 圆环半径与周长（SVG 用户单位。viewBox 16×16、描边 1.6 时外缘半径 7.4，四周留白） */
const RADIUS = 6.6;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

/** 一轮周期：弧长伸缩一次 + 旋转一圈。同周期让"收束"不必依赖人为的最短可见时长 */
const CYCLE_MS = 1200;
/** 入场：弧长长到循环态的最小弧长 */
const ENTER_MS = 200;
/** 退场：弧长从当前值收到 0 */
const SETTLE_MS = 280;
/** 旋转角速度（度/毫秒）：与周期绑定，保证一个周期恰好转一圈 */
const OMEGA = 360 / CYCLE_MS;
/** 循环态的弧长区间（占整圈的比例）：不取 0，保证工作期间环持续可见、不闪断 */
const ARC_MIN = 0.12;
const ARC_MAX = 0.7;

type Phase = 'idle' | 'entering' | 'looping' | 'settling';

/** 弧长（整圈的比例）、弧起点沿圆周的位置（整圈的比例）、旋转角（度）、整体不透明度 */
const arc = ref(0);
const arcStart = ref(0);
const angle = ref(0);
const opacity = ref(0);

let phase: Phase = 'idle';
/** 当前阶段的起始时间戳 */
let phaseStart = 0;
/** 循环态的旋转基准 */
let loopStart = 0;
let loopAngleBase = 0;
/** 入场/收束的起手值：从当前姿态续起，中途被打断（收束未播完又重新激活）时不跳变 */
let enterFromArc = 0;
let enterFromAngle = 0;
let settleFromArc = 0;
let settleFromStart = 0;
let settleFromAngle = 0;
let rafId = 0;

function easeOutCubic(t: number): number {
    return 1 - (1 - t) ** 3;
}

/** 两端导数为零的平滑起止：收束用它，避免尾巴起手/收尾出现速度突变 */
function smoothstep(t: number): number {
    return t * t * (3 - 2 * t);
}

function scheduleFrame(): void {
    if (rafId === 0) {
        rafId = requestAnimationFrame(renderFrame);
    }
}

function stopFrames(): void {
    if (rafId !== 0) {
        cancelAnimationFrame(rafId);
        rafId = 0;
    }
}

function renderFrame(now: number): void {
    rafId = 0; // 本帧已触发，下一帧由各分支末尾重新排入
    const elapsed = now - phaseStart;

    if (phase === 'entering') {
        const t = Math.min(elapsed / ENTER_MS, 1);
        arc.value = enterFromArc + (ARC_MIN - enterFromArc) * easeOutCubic(t);
        angle.value = enterFromAngle + OMEGA * elapsed;
        if (t >= 1) {
            phase = 'looping';
            loopStart = now;
            loopAngleBase = angle.value;
        }
        scheduleFrame();
        return;
    }

    if (phase === 'looping') {
        const loopElapsed = now - loopStart;
        // 升余弦：一个周期内平滑地伸长再缩短，两端导数为零（无折角）
        const swell = 0.5 - 0.5 * Math.cos((2 * Math.PI * (loopElapsed % CYCLE_MS)) / CYCLE_MS);
        arc.value = ARC_MIN + (ARC_MAX - ARC_MIN) * swell;
        angle.value = loopAngleBase + OMEGA * loopElapsed;
        scheduleFrame();
        return;
    }

    if (phase === 'settling') {
        const t = Math.min(elapsed / SETTLE_MS, 1);
        const eased = smoothstep(t);
        // 尾巴沿路径向前扫过、头部停在原处：两者都朝顺时针走，弧长顺势收到 0
        arcStart.value = settleFromStart + settleFromArc * eased;
        arc.value = settleFromArc * (1 - eased);
        // 旋转同时轻微减速（角速度取 OMEGA·(1 - 0.6t)，积分得下式的角度增量）
        angle.value = settleFromAngle + OMEGA * SETTLE_MS * (t - 0.3 * t * t);
        // 圆头描边在弧长趋零时仍会残留一个点，故在弧长已近乎不可见的末段整体淡出，
        // 让"消失"是干净的（不依赖是否恰好收到 0）
        opacity.value = t < 0.7 ? 1 : 1 - easeOutCubic((t - 0.7) / 0.3);
        if (t >= 1) {
            phase = 'idle';
            arc.value = 0;
            opacity.value = 0;
            return; // 收束完成，不再排下一帧
        }
        scheduleFrame();
    }
}

/** 开始工作指示 */
function start(): void {
    // 弧起点归一化到路径原点：面板角上只有把起点收在原点附近，弧才不会越过路径终点
    // 被截断。起点后移的等量位移补偿到旋转角上（屏上角度 = 旋转 + 起点角），位置不变
    if (arcStart.value !== 0) {
        angle.value += arcStart.value * 360;
        arcStart.value = 0;
    }
    enterFromArc = arc.value;
    enterFromAngle = angle.value;
    phase = 'entering';
    phaseStart = performance.now();
    opacity.value = 1;
    scheduleFrame();
}

/** 收束退场：尾巴顺旋转方向追上来，弧长归零后自行隐藏 */
function settle(): void {
    if (phase === 'idle' || phase === 'settling') {
        return;
    }
    settleFromArc = arc.value;
    settleFromStart = arcStart.value;
    settleFromAngle = angle.value;
    phase = 'settling';
    phaseStart = performance.now();
    scheduleFrame();
}

watch(() => props.active, (active) => {
    if (active) {
        start();
    } else {
        settle();
    }
}, { immediate: true });

onBeforeUnmount(stopFrames);
</script>

<template>
    <svg class="sentence-capture-ring" :style="{ transform: `rotate(${angle}deg)`, opacity }" width="16"
        height="16" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <circle cx="8" cy="8" :r="RADIUS" fill="none"
            :stroke-dasharray="`${(arc * CIRCUMFERENCE).toFixed(2)} ${CIRCUMFERENCE.toFixed(2)}`"
            :stroke-dashoffset="(-arcStart * CIRCUMFERENCE).toFixed(2)" />
    </svg>
</template>

<style scoped>
.sentence-capture-ring {
    /* 钉在句子面板（白色圆角面板）右下角内侧，整个环落在面板内：
       .sentence-container 的右 padding 7.5px、下 padding 15px 正是面板右边框/下边框的位置，
       各再加 5px 内缩。绝对定位相对 .sentence-container 而非面板——面板自身是滚动容器
       （overflow-y: auto），放进去会随内容滚走并被裁剪 */
    position: absolute;
    right: calc(15px / 2 + 5px);
    bottom: calc(15px + 5px);
    /* 纯展示：不参与命中测试，避免干扰面板上的词元拖刷/点击手势 */
    pointer-events: none;
    transform-origin: center;
    /* 安静的中性色（不用高饱和 accent）：浅色下是中性灰，深色下是半透明白 */
    color: #8a8a8a;
}

:global(html.dark) .sentence-capture-ring {
    color: #ffffff73;
}

.sentence-capture-ring circle {
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
}
</style>
