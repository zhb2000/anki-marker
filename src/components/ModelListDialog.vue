<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { FluentDialogShell, FluentInput, FluentToggleSwitch } from '../fluent-controls';
import { isLikelyNonChatModel, type RemoteModelInfo } from '../logics/llm';

/**
 * 模型列表选择弹窗（WinUI ContentDialog 风格）。
 *
 * 展示从 GET /models 拉取的远端模型列表：顶部搜索（匹配模型 id 与归属方 owned_by）+
 * “显示全部”开关（默认隐藏疑似非对话模型，见 isLikelyNonChatModel；仅在确实存在被隐藏
 * 条目时才展示该开关），点击条目即选中回填。
 * 加载/错误/空态由父组件通过 props 传入；刷新与关闭通过事件交回父组件处理（拉取与缓存放
 * 在设置页侧，本组件保持无副作用）。弹窗框架由 FluentDialogShell 提供。
 */

const props = defineProps<{
    /** 弹窗是否可见（挂载/卸载由该值驱动） */
    open: boolean;
    /** 模型列表拉取中 */
    loading: boolean;
    /** 拉取失败的错误描述（null 表示无错误） */
    error: string | null;
    /** 远端模型列表（已去重排序） */
    models: RemoteModelInfo[];
    /** 当前已配置的模型名，用于列表高亮 */
    currentModel: string;
}>();

const emit = defineEmits<{
    close: [];
    /** 点击“重新获取”：由父组件重新发起拉取 */
    refresh: [];
    /** 点击某个模型条目，参数为模型 id */
    select: [id: string];
}>();

const search = ref('');
/** 是否关闭启发式过滤显示全部模型；每次打开弹窗重置为默认过滤 */
const showAll = ref(false);

/** 依次应用启发式过滤与搜索过滤（搜索同时匹配 id 与 owned_by） */
const visibleModels = computed(() => {
    const query = search.value.trim().toLowerCase();
    return props.models.filter(model => {
        if (!showAll.value && isLikelyNonChatModel(model.id)) {
            return false;
        }
        if (query.length > 0
            && !model.id.toLowerCase().includes(query)
            && !(model.ownedBy?.toLowerCase().includes(query) ?? false)) {
            return false;
        }
        return true;
    });
});

/** 列表中是否存在会被启发式过滤隐藏的模型（决定“显示全部”开关是否展示） */
const hasHiddenModels = computed(() => props.models.some(model => isLikelyNonChatModel(model.id)));

/** 被启发式过滤隐藏的模型数量（有隐藏时在列表下方提示，配合“显示全部”开关） */
const hiddenCount = computed(() =>
    showAll.value ? 0 : props.models.filter(model => isLikelyNonChatModel(model.id)).length
);

const commands = computed(() => [
    // “重新获取”是本弹窗的主操作（ContentDialog PrimaryButton，居左），用 Accent 强调
    { key: 'refresh', label: '重新获取', accent: true, disabled: props.loading },
    { key: 'close', label: '关闭' },
]);

function onCommand(key: string): void {
    if (key === 'refresh') {
        emit('refresh');
    } else if (key === 'close') {
        emit('close');
    }
}

// #region 打开时的重置与聚焦（Esc/遮罩关闭由 FluentDialogShell 负责）
const searchRef = ref<{ $el: HTMLElement } | null>(null);

function focusSearch(): void {
    const el = searchRef.value?.$el;
    // clearable 模式下 $el 是容器 div，非 clearable 时就是 input 本身
    const input = el?.tagName === 'INPUT' ? el : el?.querySelector('input');
    input?.focus();
}

watch(() => props.open, open => {
    if (open) {
        search.value = '';
        showAll.value = false;
        void nextTick(() => focusSearch());
    }
});
// #endregion
</script>

<template>
    <FluentDialogShell :open="open" title="获取模型列表" :commands="commands"
        @close="emit('close')" @command="onCommand">
        <div class="dialog-toolbar">
            <FluentInput ref="searchRef" class="search-input" placeholder="搜索模型或归属方"
                clearable v-model="search" />
            <!-- 仅在列表加载成功时展示：加载中/失败时 models 可能是残留数据，无从判断是否存在隐藏条目 -->
            <label v-if="!loading && error == null && hasHiddenModels" class="show-all">
                <FluentToggleSwitch v-model="showAll" />
                <span>显示全部</span>
            </label>
        </div>
        <div class="list-area">
            <div v-if="loading" class="state-hint">正在获取模型列表…</div>
            <div v-else-if="error != null" class="state-error">
                <div class="state-error-title">获取模型列表失败</div>
                <div class="state-error-detail">{{ error }}</div>
                <div class="state-error-hint">
                    可点击下方“重新获取”重试；也可以直接关闭弹窗手动输入模型名。
                </div>
            </div>
            <div v-else-if="visibleModels.length === 0" class="state-hint">
                {{ models.length === 0 ? '服务未返回任何模型' : '没有匹配的模型' }}
            </div>
            <ul v-else class="model-list">
                <li v-for="model in visibleModels" :key="model.id">
                    <button type="button" class="model-item"
                        :class="{ selected: model.id === currentModel }" :title="model.id"
                        @click="emit('select', model.id)">
                        <span class="model-text">
                            <span class="model-id">{{ model.id }}</span>
                            <span v-if="model.ownedBy" class="model-owned">{{ model.ownedBy }}</span>
                        </span>
                    </button>
                </li>
            </ul>
        </div>
        <!-- 仅在列表加载成功时提示隐藏数量：加载中/失败时 models 可能是上次成功拉取的残留数据 -->
        <div v-if="!loading && error == null && hiddenCount > 0" class="filter-hint">
            已按启发式规则隐藏 {{ hiddenCount }} 个疑似非对话模型（嵌入/语音/绘图等）
        </div>
    </FluentDialogShell>
</template>

<style scoped>
.dialog-toolbar {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    gap: 16px;
    padding-bottom: 12px;
}

.search-input {
    flex: 1 1 auto;
    min-width: 0;
    height: 32px;
}

.show-all {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    gap: 8px;
    font-size: 14px;
    user-select: none;
    cursor: default;
}

.list-area {
    flex: 1 1 auto;
    min-height: 160px;
    max-height: 320px;
    overflow-y: auto;
}

/* 加载/空态与错误态提示 */
.state-hint {
    padding: 32px 0;
    font-size: 14px;
    text-align: center;
    opacity: 0.6;
    user-select: none;
}

.state-error {
    margin: 8px 0;
    padding: 12px;
    border: 1px solid var(--critical-fill-color);
    border-radius: var(--border-radius);
}

.state-error-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--critical-fill-color);
}

.state-error-detail {
    margin-top: 4px;
    font-size: 12px;
    word-break: break-all;
    white-space: pre-wrap;
}

.state-error-hint {
    margin-top: 8px;
    font-size: 12px;
    opacity: 0.6;
}

.model-list {
    margin: 0;
    padding: 0;
    list-style: none;
}

.model-item {
    position: relative;
    display: flex;
    align-items: center;
    width: 100%;
    margin-bottom: 2px;
    padding: 6px 10px;
    border: none;
    border-radius: 4px;
    background-color: transparent;
    color: inherit;
    font-family: var(--font-family);
    text-align: left;
}

/* 选中项（当前配置的模型）：对齐 FluentSelect 下拉项——Subtle 底色 + 左侧 accent 指示条。
   置于 :hover 之前，使悬停底色能覆盖选中底色（与下拉项 highlighted 行为一致） */
.model-item.selected {
    background-color: var(--flyout-item-background-selected);
}

/* 选中指示条：对齐 FluentSelect 下拉项的 Pill（3×16、圆角 1.5、AccentFill、
   垂直居中于条目左缘，仅选中态可见） */
.model-item::before {
    content: '';
    position: absolute;
    left: 0;
    top: calc(50% - 8px);
    width: 3px;
    height: 16px;
    border-radius: 1.5px;
    background-color: var(--accent);
    opacity: 0;
    pointer-events: none;
}

.model-item.selected::before {
    opacity: 1;
}

/* 按下选中项时指示条纵向压缩（对齐 WinUI SelectedPressed 的 Pill 缩放，0.167s） */
.model-item.selected:active::before {
    transform: scaleY(0.625);
    transition: transform 0.167s cubic-bezier(0, 0, 0, 1);
}

.model-item:hover {
    background-color: var(--flyout-item-background-hover);
}

.model-item:active {
    background-color: var(--flyout-item-background-active);
}

.model-item:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: -2px;
}

.model-text {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-width: 0;
}

.model-id {
    overflow: hidden;
    font-size: 14px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.model-owned {
    overflow: hidden;
    font-size: 12px;
    opacity: 0.55;
    text-overflow: ellipsis;
    white-space: nowrap;
}

/* 被启发式过滤隐藏的数量提示 */
.filter-hint {
    flex-shrink: 0;
    padding: 8px 0 12px;
    font-size: 12px;
    opacity: 0.55;
    user-select: none;
}
</style>
