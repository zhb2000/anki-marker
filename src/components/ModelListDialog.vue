<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { FluentInput, FluentToggleSwitch } from '../fluent-controls';
import { isLikelyNonChatModel, type RemoteModelInfo } from '../logics/llm';

/**
 * 模型列表选择弹窗（WinUI ContentDialog 风格）。
 *
 * 展示从 GET /models 拉取的远端模型列表：顶部搜索（匹配模型 id 与归属方 owned_by）+
 * “显示全部”开关（默认隐藏疑似非对话模型，见 isLikelyNonChatModel），点击条目即选中回填。
 * 加载/错误/空态由父组件通过 props 传入；刷新与关闭通过事件交回父组件处理（拉取与缓存放
 * 在设置页侧，本组件保持无副作用）。
 *
 * 视觉规格对齐 WinUI ContentDialog：smoke 遮罩（30% 黑）、弹层用 flyout token（8px 圆角、
 * SurfaceStroke 描边、双层阴影）、底部通栏命令条（44px 高、中缝 1px 分隔、无圆角的扁平按钮）。
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

/** 被启发式过滤隐藏的模型数量（有隐藏时在列表下方提示，配合“显示全部”开关） */
const hiddenCount = computed(() =>
    showAll.value ? 0 : props.models.filter(model => isLikelyNonChatModel(model.id)).length
);

// #region 打开/关闭的生命周期：重置过滤条件、聚焦搜索框、Esc 关闭
const searchRef = ref<{ $el: HTMLElement } | null>(null);

function focusSearch(): void {
    const el = searchRef.value?.$el;
    // clearable 模式下 $el 是容器 div，非 clearable 时就是 input 本身
    const input = el?.tagName === 'INPUT' ? el : el?.querySelector('input');
    input?.focus();
}

function onKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
        emit('close');
    }
}

watch(() => props.open, open => {
    if (open) {
        search.value = '';
        showAll.value = false;
        void nextTick(() => focusSearch());
        window.addEventListener('keydown', onKeydown);
    } else {
        window.removeEventListener('keydown', onKeydown);
    }
});

onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown));
// #endregion
</script>

<template>
    <Teleport to="body">
        <!-- mousedown.self：仅点击遮罩空白处关闭，弹窗内部的点击不受影响 -->
        <div v-if="open" class="model-dialog-backdrop" @mousedown.self="emit('close')">
            <div class="model-dialog" role="dialog" aria-modal="true" aria-label="获取模型列表">
                <div class="dialog-title">获取模型列表</div>
                <div class="dialog-body">
                    <div class="dialog-toolbar">
                        <FluentInput ref="searchRef" class="search-input" placeholder="搜索模型或归属方"
                            clearable v-model="search" />
                        <label class="show-all">
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
                                    <span v-if="model.id === currentModel" class="model-check">✓</span>
                                </button>
                            </li>
                        </ul>
                    </div>
                    <div v-if="hiddenCount > 0" class="filter-hint">
                        已按启发式规则隐藏 {{ hiddenCount }} 个疑似非对话模型（嵌入/语音/绘图等）
                    </div>
                </div>
                <div class="dialog-commands">
                    <button type="button" class="command-button" :disabled="loading" @click="emit('refresh')">
                        重新获取
                    </button>
                    <button type="button" class="command-button command-separator" @click="emit('close')">关闭</button>
                </div>
            </div>
        </div>
    </Teleport>
</template>

<style scoped>
/* 遮罩：WinUI ContentDialog 的 smoke 背板（30% 黑） */
.model-dialog-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: grid;
    place-items: center;
    background-color: rgba(0, 0, 0, 0.3);
}

/* 弹层容器：flyout token（8px 圆角/SurfaceStroke 描边/双层阴影），纵向布局；
   宽度对齐 ContentDialog 的常规尺寸，超高时整体收缩，内部列表自行滚动 */
.model-dialog {
    display: flex;
    flex-direction: column;
    width: min(480px, calc(100vw - 48px));
    max-height: calc(100vh - 48px);
    overflow: hidden;
    color: var(--control-text-color);
    font-family: var(--font-family);
    background-color: var(--flyout-background);
    border: 1px solid var(--flyout-border-color);
    border-radius: var(--flyout-border-radius);
    box-shadow: var(--flyout-shadow);
}

.dialog-title {
    flex-shrink: 0;
    padding: 20px 24px 12px;
    font-size: 20px;
    font-weight: 600;
    user-select: none;
}

.dialog-body {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    padding: 0 24px;
}

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
    cursor: pointer;
}

.model-item:hover {
    background-color: var(--flyout-item-background-hover);
}

.model-item:active {
    background-color: var(--flyout-item-background-active);
}

/* 选中项：当前配置的模型，WinUI 列表选中项底色 + accent 对勾 */
.model-item.selected {
    background-color: var(--flyout-item-background-selected);
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

.model-check {
    flex-shrink: 0;
    padding-left: 8px;
    color: var(--accent);
    font-size: 14px;
}

/* 被启发式过滤隐藏的数量提示 */
.filter-hint {
    flex-shrink: 0;
    padding: 8px 0 12px;
    font-size: 12px;
    opacity: 0.55;
    user-select: none;
}

/* 底部命令条：ContentDialog 规格——44px 高、通栏对分、中缝 1px 分隔、扁平无圆角 */
.dialog-commands {
    display: grid;
    flex-shrink: 0;
    grid-template-columns: 1fr 1fr;
    height: 44px;
    margin-top: 12px;
    border-top: 1px solid var(--border-color);
}

.command-button {
    border: none;
    background-color: transparent;
    color: var(--control-text-color);
    font-family: var(--font-family);
    font-size: 14px;
    cursor: pointer;
}

.command-button:hover {
    background-color: var(--flyout-item-background-hover);
}

.command-button:active {
    background-color: var(--flyout-item-background-active);
}

.command-button:disabled {
    color: var(--control-text-color-disabled);
    cursor: default;
}

.command-button:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: -2px;
}

.command-separator {
    border-left: 1px solid var(--border-color);
}
</style>
