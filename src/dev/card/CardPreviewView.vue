<!--
  卡片模板预览：把 model-front/back/css 当成 Anki 会渲染的样子跑起来，
  左右并排看正面与背面，可切样本、宽度与夜间模式。

  模板的脚本、朗读与折叠都按原样执行，只有两处例外：链接不真的跳转（会把 iframe 导航走）、
  [sound:] 字段没有真实媒体可放。Anki 注入的基础样式只做了近似，发布前仍要在 Anki 里过一眼。
-->
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import MODEL_BACK from '../../assets/model-back.html?raw';
import MODEL_CSS from '../../assets/model-css.css?raw';
import MODEL_FRONT from '../../assets/model-front.html?raw';
import { SAMPLES } from './fixtures';
import { renderTemplate } from './render';

/* ---------- 舞台：一个接近 Anki 复习界面的空白页 ---------- */

/** Anki 在卡片 CSS 之外注入的基础样式；只还原影响版面的部分（取值是近似） */
const STAGE_BASE_CSS = `
html { margin: 0; }
body { margin: 0; background: #ffffff; }
body.nightMode { background: #2f2f31; }
#qa { padding: 12px 12px 20px; }
/* Anki 把 [sound:x] 换成自带的播放按钮；这里给一个形状相近的替代 */
.replay-button.soundLink {
    display: inline-block;
    width: 1em;
    height: 1em;
    line-height: 1em;
    border: 1px solid #888;
    border-radius: 50%;
    text-align: center;
    font-size: 10px;
    color: #333;
    text-decoration: none;
    vertical-align: middle;
}
.nightMode .replay-button.soundLink { border-color: #999; color: #ddd; }
`;

// SFC 的 script 块不能出现字面的结束标签，拆开拼
const SCRIPT_OPEN = '<script>';
const SCRIPT_CLOSE = '<' + '/script>';

function buildShell(night: boolean): string {
    return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>${STAGE_BASE_CSS}
${MODEL_CSS}</style>
</head>
<body class="card${night ? ' nightMode' : ''}">
<div id="qa"></div>
${SCRIPT_OPEN}
window.__errors = [];
window.onerror = function (message) { window.__errors.push(String(message)); };
window.addEventListener('unhandledrejection', function (event) {
    window.__errors.push('unhandledrejection: ' + String(event.reason));
});
var originalError = console.error;
console.error = function () {
    window.__errors.push(Array.prototype.map.call(arguments, String).join(' '));
    originalError.apply(console, arguments);
};
// 换面时 Anki 会重新创建 script 元素让模板脚本再跑一遍；innerHTML 插入的脚本不会自动执行，
// 这里照 Anki 的做法克隆一份替换，顺带复刻了“脚本重复执行”这个真机上才会踩到的场景
window.__setHtml = function (html) {
    var qa = document.getElementById('qa');
    qa.innerHTML = html;
    Array.prototype.forEach.call(qa.querySelectorAll('script'), function (old) {
        if (old.type && old.type !== 'text/javascript') {
            return;
        }
        var script = document.createElement('script');
        Array.prototype.forEach.call(old.attributes, function (attr) {
            script.setAttribute(attr.name, attr.value);
        });
        script.textContent = old.textContent;
        old.parentNode.replaceChild(script, old);
    });
};
${SCRIPT_CLOSE}
</body>
</html>`;
}

/* ---------- 页面状态 ---------- */

const WIDTHS = [
    { value: 800, label: '桌面 800' },
    { value: 390, label: 'iPhone 390' },
    { value: 412, label: 'Android 412' },
];

const sampleId = ref(SAMPLES[0].id);
const width = ref(800);
const night = ref(false);
const renderKey = ref(0);

const sample = computed(() => SAMPLES.find(item => item.id === sampleId.value) ?? SAMPLES[0]);
const shell = computed(() => buildShell(night.value));
const frontHtml = computed(() => renderTemplate(MODEL_FRONT, sample.value.fields));
const backHtml = computed(() => renderTemplate(MODEL_BACK, sample.value.fields, frontHtml.value));

/** 舞台 shell 挂在 iframe window 上的东西 */
interface StageWindow extends Window {
    __errors?: string[];
    __setHtml: (html: string) => void;
    /** 模板脚本挂在 window 上的跨次状态，脚本没跑起来时不存在 */
    __ankiMarkerCardState?: { audio?: { src?: string; paused?: boolean; } };
}

const frontFrame = ref<HTMLIFrameElement | null>(null);
const backFrame = ref<HTMLIFrameElement | null>(null);
const errors = ref<string[]>([]);
/** 最近一次点击的链接与要播放的音频地址：预览里都不真的跳转/联网，只在这里报出来 */
const linkNotice = ref<string | null>(null);
/** 模板最近一次选中的朗读地址；只是记录，不代表当前是否在播 */
const audioNotice = ref<string | null>(null);

let observers: ResizeObserver[] = [];
let errorTimer: number | undefined;

function disconnectObservers() {
    observers.forEach(observer => observer.disconnect());
    observers = [];
}

/** 让 iframe 跟着内容长高，折叠动画期间也会持续更新 */
function trackHeight(frame: HTMLIFrameElement) {
    const doc = frame.contentDocument;
    if (doc == null) {
        return;
    }
    const update = () => {
        frame.style.height = `${doc.documentElement.scrollHeight}px`;
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(doc.documentElement);
    observers.push(observer);
}

/**
 * 链接在 Anki 里是交给系统浏览器打开的；预览里不能真跳，否则 iframe 会变成那个网页、
 * 整个舞台就没了。拦下来把地址显示给用户，顺带也能核对链接对不对。
 */
function guardLinks(frame: HTMLIFrameElement) {
    const doc = frame.contentDocument;
    if (doc == null) {
        return;
    }
    const handler = (event: Event) => {
        const anchor = (event.target as Element | null)?.closest('a[href]');
        if (anchor == null) {
            return;
        }
        event.preventDefault();
        event.stopPropagation();
        linkNotice.value = anchor.getAttribute('href');
    };
    doc.addEventListener('click', handler, true);
    doc.addEventListener('auxclick', handler, true);
}

function showHtml(frame: HTMLIFrameElement, htmls: string[]) {
    const win = frame.contentWindow as StageWindow | null;
    if (win == null) {
        return;
    }
    win.__setHtml(htmls[0]);
    if (htmls.length > 1) {
        // 先铺正面再换背面：真机上模板脚本就是在同一个 window 里执行两次的
        window.setTimeout(() => win.__setHtml(htmls[1]), 0);
    }
    trackHeight(frame);
    guardLinks(frame);
}

function onFrontLoad() {
    if (frontFrame.value != null) {
        showHtml(frontFrame.value, [frontHtml.value]);
    }
}

function onBackLoad() {
    if (backFrame.value != null) {
        showHtml(backFrame.value, [frontHtml.value, backHtml.value]);
    }
}

/** 每 500ms 从两个舞台取一次报错与模板选中的朗读地址 */
function pollStage() {
    const found = new Set<string>();
    let playedSrc = '';
    for (const frame of [frontFrame.value, backFrame.value]) {
        try {
            const win = frame?.contentWindow as StageWindow | null;
            win?.__errors?.forEach((item: string) => found.add(item));
            const state = win?.__ankiMarkerCardState?.audio;
            // 正在播放的那个优先，否则取最后设置过的（两个舞台各有一份 Audio）
            if (state?.src != null && state.src !== '' && (playedSrc === '' || state.paused === false)) {
                playedSrc = state.src;
            }
        } catch {
            // 文档尚未就绪：下一轮再收
        }
    }
    errors.value = [...found];
    if (playedSrc !== '') {
        audioNotice.value = playedSrc;
    }
}

function rerender() {
    disconnectObservers();
    linkNotice.value = null;
    audioNotice.value = null;
    renderKey.value += 1;
}

watch([sampleId, night], rerender);
watch(renderKey, disconnectObservers);

onMounted(() => {
    errorTimer = window.setInterval(pollStage, 500);
});

onBeforeUnmount(() => {
    window.clearInterval(errorTimer);
    disconnectObservers();
});
</script>

<template>
    <div class="preview-page">
        <div class="toolbar">
            <select v-model="sampleId" class="sample-select" title="样本内容">
                <option v-for="item in SAMPLES" :key="item.id" :value="item.id">{{ item.label }}</option>
            </select>
            <select v-model.number="width" title="预览宽度">
                <option v-for="item in WIDTHS" :key="item.value" :value="item.value">{{ item.label }}</option>
            </select>
            <label class="switch">
                <input type="checkbox" v-model="night" /> 夜间模式
            </label>
            <button @click="rerender">重新渲染</button>
            <span class="hint">改 src/assets/model-front.html / model-back.html / model-css.css 后保存即可</span>
        </div>

        <div class="panes">
            <section class="pane">
                <div class="pane-title">正面</div>
                <iframe ref="frontFrame" :key="`front-${renderKey}`" :srcdoc="shell"
                    :style="{ width: `${width}px` }" @load="onFrontLoad"></iframe>
            </section>
            <section class="pane">
                <div class="pane-title">背面</div>
                <iframe ref="backFrame" :key="`back-${renderKey}`" :srcdoc="shell"
                    :style="{ width: `${width}px` }" @load="onBackLoad"></iframe>
            </section>
        </div>

        <div v-if="linkNotice != null || audioNotice != null" class="notices">
            <div v-if="linkNotice != null">
                链接：<span class="notice-value">{{ linkNotice }}</span>（预览中不跳转）
            </div>
            <div v-if="audioNotice != null">
                朗读地址：<span class="notice-value">{{ audioNotice }}</span>
            </div>
        </div>

        <div v-if="errors.length > 0" class="errors">
            <div class="errors-title">卡片脚本报错</div>
            <div v-for="(item, index) in errors" :key="index" class="error-line">{{ item }}</div>
        </div>
    </div>
</template>

<style scoped>
.preview-page {
    height: 100vh;
    overflow: auto;
    box-sizing: border-box;
    /* macOS 覆盖式标题栏占住顶部一条，与其他页面一样让出这段高度 */
    padding: calc(12px + var(--titlebar-inset)) 16px 24px;
    background-color: #f3f3f3;
    font-family: -apple-system, "Segoe UI", sans-serif;
    font-size: 13px;
    color: #1f1f1f;
}

.toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
}

.toolbar select,
.toolbar button {
    height: 28px;
    padding: 0 8px;
    border: 1px solid #c8c8c8;
    border-radius: 4px;
    background-color: #ffffff;
    font-size: 13px;
    color: inherit;
}

.sample-select {
    min-width: 320px;
}

.switch {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    user-select: none;
}

.hint {
    color: #6b6b6b;
}

.panes {
    display: flex;
    align-items: flex-start;
    gap: 16px;
}

.pane-title {
    margin-bottom: 6px;
    color: #6b6b6b;
}

.pane iframe {
    display: block;
    width: 100%;
    height: 200px;
    border: 1px solid #d6d6d6;
    border-radius: 6px;
    background-color: #ffffff;
}

.notices {
    margin-top: 12px;
    line-height: 1.7;
    color: #5a5a5a;
}

.notice-value {
    font-family: ui-monospace, Menlo, monospace;
    word-break: break-all;
    color: #1f1f1f;
}

.errors {
    margin-top: 16px;
    padding: 10px 12px;
    border: 1px solid #e0b4b4;
    border-radius: 6px;
    background-color: #fdf3f3;
    color: #8a1f1f;
}

.errors-title {
    margin-bottom: 6px;
    font-weight: 600;
}

.error-line {
    white-space: pre-wrap;
    word-break: break-all;
    font-family: ui-monospace, Menlo, monospace;
    font-size: 12px;
}
</style>
