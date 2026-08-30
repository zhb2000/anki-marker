<script lang="ts">
export interface FluentSelectOption {
    value: string;
    label: string;
    disabled?: boolean;
}
</script>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onDeactivated, ref, useAttrs, useSlots, watch } from 'vue';
import { useRoute } from 'vue-router';
import { HoverWrapper } from './HoverWrapper';
import { generateUniqueId } from './generateUniqueId';

// 与 FluentPasswordInput 一致：关闭 attr 自动继承，class/style 手动绑到按钮
// （继承外部的尺寸设置），其余 attrs（name 等）透传给内部 button
defineOptions({ inheritAttrs: false });

const props = withDefaults(defineProps<{
    /** 选项列表；也可直接使用 <option> 插槽写法（见 parseSlotOptions），options 属性优先 */
    options?: FluentSelectOption[];
    /** 无选中项时显示的占位文本（对齐 WinUI ComboBox 的 PlaceholderText） */
    placeholder?: string;
    disabled?: boolean;
}>(), {
    options: undefined,
    placeholder: '',
    disabled: false,
});

const model = defineModel<string>();

const emit = defineEmits<{
    'change': [value: string];
}>();

const attrs = useAttrs();
const slots = useSlots();
const route = useRoute();

/** 除 class/style 外的透传属性；每次渲染时调用以读取最新 attrs */
function restAttrs() {
    const { class: _class, style: _style, ...rest } = attrs;
    return rest;
}

/** 解析 <option value="...">文本</option> 插槽写法（兼容旧的原生 select 用法）。
    插槽内容不会被真正渲染；假定其是静态的，动态选项请使用 options 属性 */
function parseSlotOptions(): FluentSelectOption[] {
    const vnodes = slots.default?.() ?? [];
    const result: FluentSelectOption[] = [];
    for (const vnode of vnodes) {
        if (vnode.type !== 'option') {
            continue;
        }
        const vnodeProps = (vnode.props ?? {}) as Record<string, unknown>;
        const children = vnode.children;
        const label = typeof children === 'string' ? children.trim() : '';
        const rawValue = vnodeProps.value;
        const value = typeof rawValue === 'string' || typeof rawValue === 'number' ? String(rawValue) : label;
        const disabled = vnodeProps.disabled === true || vnodeProps.disabled === '';
        result.push({ value, label, disabled });
    }
    return result;
}

const items = computed<FluentSelectOption[]>(() => props.options ?? parseSlotOptions());

const selectedIndex = computed(() => items.value.findIndex(item => item.value === model.value));
const selectedLabel = computed(() => selectedIndex.value >= 0 ? items.value[selectedIndex.value].label : undefined);
const firstEnabledIndex = computed(() => items.value.findIndex(item => item.disabled !== true));

/** 与 CSS 中列表项高度保持一致（弹层定位需要预先知道项高） */
const ITEM_HEIGHT = 32;
/** 列表内边距（与 .fluent-select-list 的 padding 保持一致） */
const LIST_PADDING = 4;
/** 弹层与视口边缘的最小间距 */
const POPUP_MARGIN = 8;
/** 弹层最大宽度，超出部分文本省略号截断 */
const MAX_POPUP_WIDTH = 480;
/** 打字搜索的累积窗口（对齐 WinUI ComboBox 的文本搜索行为） */
const TYPE_AHEAD_TIMEOUT = 800;

/** 弹层是否打开 */
const open = ref(false);
/** 弹层中高亮项的索引；高亮不等于选中，Enter/点击才提交（对齐 WinUI 默认的 Committed 触发方式） */
const highlightedIndex = ref(-1);
const buttonEl = ref<HTMLElement>();
const popupEl = ref<HTMLElement>();
const listEl = ref<HTMLElement>();
const listboxId = generateUniqueId('fluent-select-listbox');

const popupStyle = ref<Record<string, string>>({});
const listMaxHeight = ref('');
/** 弹层相对按钮的展开方向：决定开合动画的位移方向（主体在下方→向下展开，反之向上） */
const openDirection = ref<'down' | 'up'>('down');

/** 打字搜索缓冲 */
let searchBuffer = '';
let searchTimer: ReturnType<typeof setTimeout> | undefined;

function togglePopup() {
    if (open.value) {
        closePopup(true);
    } else {
        void openPopup();
    }
}

async function openPopup() {
    if (open.value || props.disabled || items.value.length === 0) {
        return;
    }
    highlightedIndex.value = selectedIndex.value >= 0 ? selectedIndex.value : firstEnabledIndex.value;
    updatePopupPosition();
    open.value = true;
    addGlobalListeners();
    await nextTick();
    // 渲染后按实际内容宽度再校正一次水平位置，并让高亮项滚动到可视区域
    updatePopupPosition();
    scrollHighlightedIntoView();
}

function closePopup(restoreFocus: boolean) {
    if (!open.value) {
        return;
    }
    open.value = false;
    removeGlobalListeners();
    if (restoreFocus) {
        buttonEl.value?.focus();
    }
}

/** 计算弹层位置：对齐 WinUI ComboBox —— 选中项（无选中时为第一项）垂直覆盖在控件上 */
function updatePopupPosition() {
    const button = buttonEl.value;
    if (button == null) {
        return;
    }
    const rect = button.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;

    const contentHeight = LIST_PADDING * 2 + items.value.length * ITEM_HEIGHT;
    const maxHeight = Math.min(contentHeight, viewportHeight - POPUP_MARGIN * 2);
    // 让锚点项的中心与按钮中心对齐，再钳制到视口内
    const anchorIndex = Math.max(highlightedIndex.value, 0);
    let top = rect.top + rect.height / 2 - (LIST_PADDING + anchorIndex * ITEM_HEIGHT + ITEM_HEIGHT / 2);
    const maxTop = Math.max(POPUP_MARGIN, viewportHeight - POPUP_MARGIN - maxHeight);
    top = Math.min(Math.max(top, POPUP_MARGIN), maxTop);

    const maxWidth = Math.min(MAX_POPUP_WIDTH, viewportWidth - POPUP_MARGIN * 2);
    const width = Math.min(Math.max(rect.width, popupEl.value?.offsetWidth ?? 0), maxWidth);
    const maxLeft = Math.max(POPUP_MARGIN, viewportWidth - POPUP_MARGIN - width);
    const left = Math.min(Math.max(rect.left, POPUP_MARGIN), maxLeft);

    // 依据弹层主体与按钮的相对位置判断展开方向（对齐 WinUI 从锚点向外展开的动画语义）
    openDirection.value = top + maxHeight / 2 >= rect.top + rect.height / 2 ? 'down' : 'up';

    popupStyle.value = {
        top: `${Math.round(top)}px`,
        left: `${Math.round(left)}px`,
        minWidth: `${Math.min(rect.width, maxWidth)}px`,
        maxWidth: `${maxWidth}px`,
    };
    listMaxHeight.value = `${Math.round(maxHeight)}px`;
}

/** 让高亮项滚动到可视区域（弹层被视口钳制或内容过高时需要） */
function scrollHighlightedIntoView() {
    const list = listEl.value;
    const index = highlightedIndex.value;
    if (list == null || index < 0) {
        return;
    }
    (list.children[index] as HTMLElement | undefined)?.scrollIntoView({ block: 'nearest' });
}

/** 应用选中值并触发 change（值未变化时不触发） */
function applySelection(option: FluentSelectOption) {
    if (option.value !== model.value) {
        model.value = option.value;
        emit('change', option.value);
    }
}

function commitSelection(index: number) {
    const option = items.value[index];
    if (option != null && !option.disabled) {
        applySelection(option);
    }
    closePopup(true);
}

/** 关闭状态下用方向键直接移动选中项（对齐原生 select 与 WinUI ComboBox 的键盘行为） */
function moveSelection(step: number) {
    const count = items.value.length;
    let next = selectedIndex.value < 0 ? (step > 0 ? -1 : count) : selectedIndex.value;
    next += step;
    while (next >= 0 && next < count) {
        const option = items.value[next];
        if (!option.disabled) {
            applySelection(option);
            return;
        }
        next += step;
    }
}

/** 打开状态下移动高亮项（不提交，不循环），跳过禁用项 */
function moveHighlight(step: number) {
    const count = items.value.length;
    let next = highlightedIndex.value < 0 ? (step > 0 ? -1 : count) : highlightedIndex.value;
    next += step;
    while (next >= 0 && next < count) {
        if (!items.value[next].disabled) {
            highlightedIndex.value = next;
            scrollHighlightedIntoView();
            return;
        }
        next += step;
    }
}

function highlightEdge(first: boolean) {
    const count = items.value.length;
    const range = first ? [...Array(count).keys()] : [...Array(count).keys()].reverse();
    for (const index of range) {
        if (!items.value[index].disabled) {
            highlightedIndex.value = index;
            scrollHighlightedIntoView();
            return;
        }
    }
}

function isPrintableChar(event: KeyboardEvent): boolean {
    return event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey;
}

/** 打字搜索：不区分大小写；连续按同一字符时在匹配项之间循环 */
function handleTypeAhead(char: string) {
    clearTimeout(searchTimer);
    const lower = char.toLowerCase();
    const isRepeat = searchBuffer.length > 0 && searchBuffer === lower.repeat(searchBuffer.length);
    searchBuffer = isRepeat ? lower : searchBuffer + lower;
    searchTimer = setTimeout(() => searchBuffer = '', TYPE_AHEAD_TIMEOUT);

    const currentIndex = open.value ? highlightedIndex.value : selectedIndex.value;
    const startAfter = isRepeat ? currentIndex : -1;
    const found = findByText(searchBuffer, startAfter);
    if (found < 0) {
        return;
    }
    if (open.value) {
        highlightedIndex.value = found;
        scrollHighlightedIntoView();
    } else {
        // 关闭状态下打字直接改变选中项（对齐 WinUI ComboBox）
        applySelection(items.value[found]);
    }
}

function findByText(text: string, startAfter: number): number {
    const count = items.value.length;
    for (let offset = 1; offset <= count; offset++) {
        const index = (startAfter + offset + count) % count;
        const option = items.value[index];
        if (!option.disabled && option.label.toLowerCase().startsWith(text)) {
            return index;
        }
    }
    return -1;
}

/** 关闭状态下按钮的按键处理；打开状态统一由 window 监听器处理（见 onWindowKeydown） */
function onButtonKeydown(event: KeyboardEvent) {
    if (props.disabled || open.value) {
        return;
    }
    switch (event.key) {
        case 'Enter':
        case ' ':
            // preventDefault 阻止按钮自身的 click 激活，避免与 @click 重复切换
            event.preventDefault();
            void openPopup();
            return;
        case 'ArrowDown':
        case 'ArrowUp':
            event.preventDefault();
            if (event.altKey) {
                void openPopup();
            } else {
                moveSelection(event.key === 'ArrowDown' ? 1 : -1);
            }
            return;
        default:
            if (isPrintableChar(event)) {
                handleTypeAhead(event.key);
            }
    }
}

function onWindowKeydown(event: KeyboardEvent) {
    if (!open.value) {
        return;
    }
    switch (event.key) {
        case 'ArrowDown':
        case 'ArrowUp':
            event.preventDefault();
            moveHighlight(event.key === 'ArrowDown' ? 1 : -1);
            return;
        case 'Home':
        case 'End':
            event.preventDefault();
            highlightEdge(event.key === 'Home');
            return;
        case 'Enter':
        case ' ':
            // preventDefault 阻止焦点仍在按钮上时再次触发 click 导致弹层重开
            event.preventDefault();
            commitSelection(highlightedIndex.value);
            return;
        case 'Escape':
            event.preventDefault();
            closePopup(true);
            return;
        case 'Tab':
            // 让焦点自然移动，不提交高亮项
            closePopup(false);
            return;
        default:
            if (isPrintableChar(event)) {
                event.preventDefault();
                handleTypeAhead(event.key);
            }
    }
}

function onItemPointerEnter(option: FluentSelectOption, index: number) {
    if (!option.disabled) {
        highlightedIndex.value = index;
    }
}

function onDocumentPointerDown(event: PointerEvent) {
    const target = event.target as Node | null;
    if (target == null) {
        return;
    }
    if (popupEl.value?.contains(target) === true || buttonEl.value?.contains(target) === true) {
        return;
    }
    closePopup(false);
}

function onScrollOrResize() {
    if (open.value) {
        updatePopupPosition();
    }
}

function onWindowBlur() {
    closePopup(false);
}

function addGlobalListeners() {
    document.addEventListener('pointerdown', onDocumentPointerDown, true);
    document.addEventListener('scroll', onScrollOrResize, true); // capture 阶段以捕获内部容器的滚动
    window.addEventListener('keydown', onWindowKeydown);
    window.addEventListener('resize', onScrollOrResize);
    window.addEventListener('blur', onWindowBlur);
}

function removeGlobalListeners() {
    document.removeEventListener('pointerdown', onDocumentPointerDown, true);
    document.removeEventListener('scroll', onScrollOrResize, true);
    window.removeEventListener('keydown', onWindowKeydown);
    window.removeEventListener('resize', onScrollOrResize);
    window.removeEventListener('blur', onWindowBlur);
}

watch(() => route.name, () => closePopup(false));
onDeactivated(() => closePopup(false)); // KeepAlive 切换页面时关闭弹层
onBeforeUnmount(() => {
    removeGlobalListeners();
    clearTimeout(searchTimer);
});
</script>

<template>
    <HoverWrapper>
        <button ref="buttonEl" type="button" class="fluent-select"
            :class="[$attrs.class, { empty: selectedLabel == null, open }]" :style="$attrs.style" v-bind="restAttrs()"
            :disabled="disabled" aria-haspopup="listbox" :aria-expanded="open" :aria-controls="listboxId"
            :aria-activedescendant="open && highlightedIndex >= 0 ? `${listboxId}-option-${highlightedIndex}` : undefined"
            @click="togglePopup" @keydown="onButtonKeydown">
            <span class="fluent-select-text">{{ selectedLabel ?? placeholder }}</span>
            <!-- WinUI ComboBox 的 ChevronDown 箭头（内联 SVG，不依赖系统图标字体） -->
            <svg class="fluent-select-chevron" viewBox="0 0 12 12" aria-hidden="true">
                <path d="M2 4.5 L6 8.5 L10 4.5" fill="none" stroke="currentColor" stroke-width="1.2"
                    stroke-linecap="round" stroke-linejoin="round" />
            </svg>
        </button>
    </HoverWrapper>
    <Teleport to="body">
        <Transition name="fluent-select-flyout">
            <div v-if="open" ref="popupEl" class="fluent-select-flyout"
                :class="{ 'open-up': openDirection === 'up' }" :style="popupStyle">
                <ul :id="listboxId" ref="listEl" role="listbox" class="fluent-select-list"
                    :style="{ maxHeight: listMaxHeight }">
                    <li v-for="(option, index) in items" :key="option.value" :id="`${listboxId}-option-${index}`"
                        role="option" :aria-selected="index === selectedIndex"
                        :aria-disabled="option.disabled === true" class="fluent-select-item"
                        :class="{ highlighted: index === highlightedIndex, selected: index === selectedIndex, disabled: option.disabled }"
                        @mousedown.prevent @pointerenter="onItemPointerEnter(option, index)"
                        @click="commitSelection(index)">
                        {{ option.label }}
                    </li>
                </ul>
            </div>
        </Transition>
    </Teleport>
</template>

<style scoped>
.fluent-select {
    display: inline-flex;
    align-items: center;
    min-height: 32px;
    padding-left: 12px;
    padding-right: 8px;
    font-family: var(--font-family);
    font-size: var(--font-size);
    color: var(--control-text-color);
    background-color: var(--control-background);
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
    user-select: none;
}

.fluent-select[fluent-hovered] {
    background-color: var(--control-background-hover);
}

.fluent-select:active {
    background-color: var(--control-background-active);
    border-bottom-color: var(--border-color);
    color: var(--control-text-color-active);
}

.fluent-select:disabled {
    background-color: var(--control-background-disabled);
    border-bottom-color: var(--border-color);
    color: var(--control-text-color-disabled);
}

.fluent-select:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: 1px;
}

.fluent-select-text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
}

.fluent-select.empty .fluent-select-text {
    color: var(--placeholder-color);
}

.fluent-select-chevron {
    flex-shrink: 0;
    width: 12px;
    height: 12px;
    margin-left: 4px;
    color: var(--control-text-color-active);
}

.fluent-select:disabled .fluent-select-chevron {
    color: var(--control-text-color-disabled);
}

/* 弹层：Teleport 到 body 的节点仍带本组件的 scoped 属性，样式照常生效 */
.fluent-select-flyout {
    position: fixed;
    z-index: 1000;
    width: max-content;
    background-color: var(--flyout-background);
    border: 1px solid var(--flyout-border-color);
    border-radius: var(--flyout-border-radius);
    box-shadow: var(--flyout-shadow);
    overflow: hidden;
}

.fluent-select-list {
    margin: 0;
    padding: 4px;
    /* 需与脚本中的 LIST_PADDING 保持一致 */
    overflow-y: auto;
    overscroll-behavior: contain;
    list-style: none;
}

.fluent-select-item {
    position: relative;
    display: flex;
    align-items: center;
    height: 32px;
    /* 需与脚本中的 ITEM_HEIGHT 保持一致 */
    padding: 0 12px;
    border-radius: 4px;
    /* 内嵌圆角高亮块（对齐 WinUI ComboBox 列表项） */
    font-family: var(--font-family);
    font-size: var(--font-size);
    color: var(--control-text-color);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    user-select: none;
    cursor: default;
}

.fluent-select-item.selected {
    background-color: var(--flyout-item-background-selected);
}

/* 选中项左侧的 accent 指示条（对齐 WinUI ComboBoxItem 模板中的 Pill：
   3×16、圆角 1.5、AccentFill、垂直居中于高亮块左缘，仅选中态可见） */
.fluent-select-item::before {
    content: '';
    position: absolute;
    left: 0;
    top: calc(50% - 8px);
    width: 3px;
    height: 16px;
    border-radius: 1.5px;
    background-color: var(--accent);
    opacity: 0;
    transform: scaleY(1);
    pointer-events: none;
}

.fluent-select-item.selected::before {
    opacity: 1;
}

/* 按下选中项时指示条纵向压缩（对齐 WinUI SelectedPressed 的 Pill 缩放，0.167s） */
.fluent-select-item.selected:active::before {
    transform: scaleY(0.625);
    transition: transform 0.167s cubic-bezier(0, 0, 0, 1);
}

.fluent-select-item.highlighted,
.fluent-select-item.selected.highlighted {
    background-color: var(--flyout-item-background-hover);
}

.fluent-select-item:active {
    background-color: var(--flyout-item-background-active);
}

.fluent-select-item.disabled {
    color: var(--control-text-color-disabled);
    background-color: transparent;
}

/* 弹层进出动画：对齐 WinUI 弹层 motion（FastOutSlowIn 缓动，打开 250ms / 关闭 150ms）。
   打开时沿展开方向从锚点外侧 8px 轻移入位，关闭时向锚点回落 4px */
.fluent-select-flyout-enter-active {
    transition: opacity 0.25s cubic-bezier(0.1, 0.9, 0.2, 1), transform 0.25s cubic-bezier(0.1, 0.9, 0.2, 1);
}

.fluent-select-flyout-leave-active {
    transition: opacity 0.15s cubic-bezier(0.1, 0.9, 0.2, 1), transform 0.15s cubic-bezier(0.1, 0.9, 0.2, 1);
}

.fluent-select-flyout-enter-from {
    opacity: 0;
    transform: translateY(-8px);
}

.fluent-select-flyout.open-up.fluent-select-flyout-enter-from {
    transform: translateY(8px);
}

.fluent-select-flyout-leave-to {
    opacity: 0;
    transform: translateY(-4px);
}

.fluent-select-flyout.open-up.fluent-select-flyout-leave-to {
    transform: translateY(4px);
}

@media (prefers-reduced-motion: reduce) {
    .fluent-select-flyout-enter-active,
    .fluent-select-flyout-leave-active {
        transition-duration: 0.1s;
    }

    .fluent-select-flyout-enter-from,
    .fluent-select-flyout-leave-to {
        transform: none;
    }
}
</style>
