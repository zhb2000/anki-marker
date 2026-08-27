<script setup lang="ts">
import { computed, ref, watch, type PropType } from 'vue';

import type { AiPickState, DictSource } from '../logics/aiPick';
import type { CollinsItem, OxfordItem, YoudaoItem } from '../logics/dict';
import AddButton from './AddButton.vue';
import type { CardStatus } from './CardStatus';

const props = defineProps({
    state: {
        type: Object as PropType<AiPickState>,
        required: true
    },
    /** 加号按钮状态，跟随 AI 选中的那条词典条目的添加状态 */
    status: {
        type: String as PropType<CardStatus>,
        default: 'not-added'
    },
    /** AI 优选命中的词典条目（含完整条目数据）；未命中或条目缺失时为 null */
    pickedItem: {
        type: Object as PropType<{ source: DictSource; item: CollinsItem | OxfordItem | YoudaoItem; } | null>,
        default: null
    },
    /** 是否为当前可见词典 tab 内的实例：仅可见实例播放入场/淡入动画（见 cardAnimIn 注释） */
    active: {
        type: Boolean,
        required: true
    }
});

const emit = defineEmits<{
    'add-btn-click': [];
}>();

/** 完成态且命中真实词典条目（非 fallback）时才显示加号，语义等同该条目卡片上的加号 */
const showAddButton = computed(() => props.state.phase === 'done' && props.state.pick != null);

function emitAddBtnClick() {
    emit('add-btn-click');
}

/** 词典来源的显示名 */
const DICT_LABELS: Record<DictSource, string> = {
    collins: '柯林斯',
    oxford: '牛津',
    youdao: '有道'
};

/** 完成态且命中词典条目时的来源名 */
const sourceLabel = computed(() => {
    const pick = props.state.pick;
    return pick != null ? DICT_LABELS[pick.source] : null;
});

/** 按来源窄化后的被优选条目，三者至多一个非空（模板按此选择对应词典的内容版式） */
const collinsItem = computed(() => props.pickedItem?.source === 'collins' ? props.pickedItem.item as CollinsItem : null);
const oxfordItem = computed(() => props.pickedItem?.source === 'oxford' ? props.pickedItem.item as OxfordItem : null);
const youdaoItem = computed(() => props.pickedItem?.source === 'youdao' ? props.pickedItem.item as YoudaoItem : null);

/**
 * 入场动画用 class 控制且只播一次，且仅“挂载时可见”的实例才播：本卡在三个 ScrollMemory
 * 容器内各一份实例，容器用 v-show 切显隐。display:none 的实例动画不会启动、animationend
 * 不触发，class 摘不掉；等它随 tab 切换变为可见时浏览器重建盒模型，仍挂着的动画 class
 * 会被当作新动画播放（即切词典 tab 时卡片闪一下）。因此只有挂载时 active 的实例才武装
 * 动画；不可见实例从始至终无 class，之后无论怎么切显隐都无动画可重放。新查词时 v-if
 * 重挂载本卡，ref 按当时的 active 重新初始化
 */
const cardAnimIn = ref(props.active);
/** body 的淡入动画同理：key 随 phase/pick 变化重挂载时，仅可见实例重播一次 */
const bodyAnimIn = ref(props.active);

watch(() => [props.state.phase, props.state.pick != null], () => {
    bodyAnimIn.value = props.active;
});

/**
 * 子元素的 animationend 会冒泡到根元素，按动画名区分，避免 body 的淡入结束误摘卡片的 class。
 * 注意：Vue scoped CSS 会给 @keyframes 名追加组件哈希（运行时实为 aiPickCardIn-45d25e2b），
 * 精确匹配会永远落空导致 class 摘不掉（切回该 tab 时动画重放），必须用前缀匹配
 */
function onCardAnimationEnd(event: AnimationEvent) {
    if (event.animationName.startsWith('aiPickCardIn')) {
        cardAnimIn.value = false;
    }
}

function onBodyAnimationEnd(event: AnimationEvent) {
    if (event.animationName.startsWith('aiPickBodyIn')) {
        bodyAnimIn.value = false;
    }
}
</script>

<template>
    <div class="ai-pick-card"
        :class="{ 'is-fallback': state.phase === 'done' && state.fallback, 'anim-card-in': cardAnimIn }"
        @animationend="onCardAnimationEnd">
        <!-- 与 WordCard 相同的左右分栏：左侧内容自适应，右侧加号按钮固定 -->
        <div class="display-flex">
            <div class="flex-grow-1">
                <div class="ai-pick-header">
                    <span class="ai-badge">AI</span>
                    <span v-if="state.phase === 'done' && sourceLabel != null" class="ai-source-badge">
                        优选 · {{ sourceLabel }}
                    </span>
                    <span v-else-if="state.phase === 'done' && state.fallback" class="ai-source-badge ai-source-warning">
                        AI 生成，非词典条目
                    </span>
                </div>
                <div v-if="state.phase === 'loading'" class="ai-skeleton" aria-hidden="true">
                    <div class="ai-skeleton-line"></div>
                    <div class="ai-skeleton-line ai-skeleton-line-short"></div>
                </div>
                <!-- key 随 phase 与 pick 是否出现变化：内容形态切换时重挂载，重放淡入动画 -->
                <div v-else class="ai-pick-body" :key="`${state.phase}:${state.pick != null}`"
                    :class="{ 'anim-body-in': bodyAnimIn }" @animationend="onBodyAnimationEnd">
                    <!-- 命中真实词典条目：渲染该条目的完整内容（内容模板复刻自三张词典卡，无卡片外壳与编辑按钮） -->
                    <template v-if="state.pick != null && collinsItem != null">
                        <b>{{ collinsItem.word }}</b>
                        <span v-if="collinsItem.sense != null"><span v-html="' '"></span><i>{{ collinsItem.sense }}</i></span>
                        <!-- 仅柯林斯的英文释义含 <b> 等 HTML 标签，用 v-html 渲染 -->
                        <div v-if="collinsItem.enDef != null" v-html="collinsItem.enDef"></div>
                        <div v-if="collinsItem.cnDef != null">{{ collinsItem.cnDef }}</div>
                    </template>
                    <template v-else-if="state.pick != null && oxfordItem != null">
                        <b>{{ oxfordItem.word }}</b>
                        <span v-if="oxfordItem.phrase != null"><span v-html="' '"></span>{{ oxfordItem.phrase }}</span>
                        <span v-if="oxfordItem.sense != null"><span v-html="' '"></span><i>{{ oxfordItem.sense }}</i></span>
                        <span v-if="oxfordItem.ext != null"><span v-html="' '"></span>{{ oxfordItem.ext }}</span>
                        <div v-if="oxfordItem.enDef != null">{{ oxfordItem.enDef }}</div>
                        <div v-if="oxfordItem.cnDef != null">{{ oxfordItem.cnDef }}</div>
                    </template>
                    <template v-else-if="state.pick != null && youdaoItem != null">
                        <b>{{ youdaoItem.word }}</b>
                        <span v-if="youdaoItem.sense != null"><span v-html="' '"></span><i>{{ youdaoItem.sense }}</i></span>
                        <div v-if="youdaoItem.cnDef != null">{{ youdaoItem.cnDef }}</div>
                    </template>
                    <!-- fallback 或 pick 尚未解析出的过渡期：打字机渲染语境化释义；
                         也兜底 pick 非空但条目数据缺失的理论分支（只显示笔记/释义区，不 crash） -->
                    <div v-else class="ai-contextual-def">
                        {{ state.contextualDef }}<span v-if="state.phase === 'streaming'" class="ai-caret"></span>
                    </div>
                    <div v-if="state.note.length > 0" class="ai-note">
                        笔记：{{ state.note }}<span
                            v-if="state.phase === 'streaming' && state.pick != null && pickedItem != null"
                            class="ai-caret"></span>
                    </div>
                </div>
            </div>
            <div v-if="showAddButton" class="flex-shrink-0">
                <AddButton :status="status" class="card-button" @click="emitAddBtnClick" />
            </div>
        </div>
    </div>
</template>

<style scoped>
.ai-pick-card {
    user-select: text;
    cursor: text;
    padding: 10px;
    /* 卡片位于滚动容器内部，与词典卡片一致只保留下边距 */
    margin-bottom: 10px;
    font-family: var(--font-family);
    font-size: var(--font-size);
    color: var(--control-text-color);
    background-color: var(--input-text-background-focus);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    /* 克制的 AI 区隔：accent 左边条 */
    border-left: 2px solid var(--accent);
    border-radius: var(--border-radius);
}

.ai-pick-card.is-fallback {
    border-left-color: var(--warning-text-color);
}

/* 入场动画不写进基础样式（见 script 中 cardAnimIn 的注释），由 class 控制且只播一次 */
.anim-card-in {
    animation: aiPickCardIn calc(0.3s * 0.5) ease-out;
}

.anim-body-in {
    animation: aiPickBodyIn calc(0.3s * 0.5) ease-out;
}

/* 以下加号按钮布局样式与 WordCard 一致 */
.display-flex {
    display: flex;
    gap: 4px;
}

.flex-grow-1 {
    flex-grow: 1;
}

.flex-shrink-0 {
    flex-shrink: 0;
}

.card-button {
    display: block;
    margin-bottom: 8px;
    width: 16px;
    height: 16px;
}

.ai-pick-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
    user-select: none;
}

.ai-badge {
    padding: 2px 5px;
    font-size: 11px;
    font-weight: 600;
    line-height: 1.4;
    border-radius: 3px;
    color: var(--control-accent-text-color);
    background-color: var(--accent);
}

.ai-source-badge {
    font-size: 12px;
    color: var(--control-text-color-active);
}

.ai-source-warning {
    color: var(--warning-text-color);
}

.ai-contextual-def {
    overflow-wrap: break-word;
}

.ai-caret {
    display: inline-block;
    width: 2px;
    height: 1em;
    margin-left: 2px;
    vertical-align: -0.15em;
    background-color: var(--accent);
    animation: aiPickPulse 1s step-end infinite;
}

.ai-note {
    margin-top: 4px;
    font-size: 13px;
    color: var(--control-text-color-active);
}

.ai-skeleton-line {
    height: 1em;
    margin: 4px 0;
    border-radius: 3px;
    background-color: var(--processing-background);
    animation: aiPickPulse 1.2s ease-in-out infinite;
}

.ai-skeleton-line-short {
    width: 60%;
}

@keyframes aiPickCardIn {
    from {
        opacity: 0;
        transform: translateY(-4px);
    }

    to {
        opacity: 1;
        transform: translateY(0);
    }
}

@keyframes aiPickBodyIn {
    from {
        opacity: 0;
        transform: translateY(-2px);
    }

    to {
        opacity: 1;
        transform: translateY(0);
    }
}

@keyframes aiPickPulse {

    0%,
    100% {
        opacity: 0.45;
    }

    50% {
        opacity: 1;
    }
}
</style>
