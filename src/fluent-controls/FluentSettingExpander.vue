<script setup lang="ts">
import { ref, watch } from 'vue';

const props = withDefaults(defineProps<{
    /** 标题 */
    header: string;
    /** 副标题/说明（标题下方，小字弱化） */
    description?: string;
    /** 搜索高亮锚点：渲染为根元素的 data-setting-id 属性，供 useHighlight 定位 */
    settingId?: string;
    /** 展开状态，支持 v-model:expanded */
    expanded?: boolean;
}>(), {
    description: undefined,
    settingId: undefined,
    expanded: false,
});

const emit = defineEmits<{
    'update:expanded': [value: boolean];
}>();

/**
 * 内部展开状态。半受控模式：
 * - 父组件用 v-model:expanded 时，emit 后父组件回写 prop，watch 同步回来（受控）
 * - 父组件不传 expanded 时，组件自行管理展开状态（非受控，展开状态通常无需外部关心）
 */
const innerExpanded = ref(props.expanded);
watch(() => props.expanded, value => {
    innerExpanded.value = value;
});

function toggle() {
    innerExpanded.value = !innerExpanded.value;
    emit('update:expanded', innerExpanded.value);
}

/** 头部行整体可点击切换展开；右侧常显控件区（header-extra）已用 @click.stop 拦截 */
function onHeaderClick() {
    toggle();
}

/** 键盘切换：仅在焦点位于头部行自身时响应，焦点在内部控件上冒泡上来的按键不处理 */
function onHeaderKeydown(event: KeyboardEvent) {
    if (event.target !== event.currentTarget) {
        return;
    }
    if (event.key === 'Enter' || event.key === ' ') {
        // 阻止空格键滚动页面
        event.preventDefault();
        toggle();
    }
}
</script>

<template>
    <div class="fluent-setting-expander" :class="{ expanded: innerExpanded }" :data-setting-id="settingId">
        <!-- 头部行：与 FluentSettingCard 视觉一致；点击非控件区域切换展开 -->
        <div class="expander-header" role="button" tabindex="0" :aria-expanded="innerExpanded"
            @click="onHeaderClick" @keydown="onHeaderKeydown">
            <!-- 标题左侧图标（可选） -->
            <div v-if="$slots.icon" class="expander-icon">
                <slot name="icon"></slot>
            </div>
            <div class="expander-main">
                <span class="expander-header-text">{{ header }}</span>
                <div v-if="description" class="expander-description">{{ description }}</div>
            </div>
            <!-- 头部右侧常显控件区（开关等）：点击不触发展开切换 -->
            <div v-if="$slots['header-extra']" class="expander-actions" @click.stop>
                <slot name="header-extra"></slot>
            </div>
            <!-- 展开箭头：与 FluentSelect 同款内联 SVG chevron，旋转过渡 -->
            <svg class="expander-chevron" viewBox="0 0 12 12" aria-hidden="true">
                <path d="M2 4.5 L6 8.5 L10 4.5" fill="none" stroke="currentColor" stroke-width="1.2"
                    stroke-linecap="round" stroke-linejoin="round" />
            </svg>
        </div>
        <!-- 展开区：grid-template-rows 0fr→1fr 过渡实现顺滑展开/收起 -->
        <div class="expander-content">
            <div class="expander-content-inner">
                <!-- 与头部的 1px 分隔线（收起时随内容一起被裁掉） -->
                <div class="expander-divider"></div>
                <div class="expander-body">
                    <!-- 默认 slot：内嵌的 FluentSettingCard 列表 -->
                    <slot></slot>
                </div>
            </div>
        </div>
    </div>
</template>

<style scoped>
.fluent-setting-expander {
    font-family: var(--font-family);
    color: var(--control-text-color);
    background-color: var(--control-background);
    border: 1px solid var(--border-color);
    /* 整体是一个圆角卡片：头部 + 展开区都在其中 */
    border-radius: 8px;
    /* 卡片自身不设外边距，间距由父容器 gap 控制 */
}

.expander-header {
    display: flex;
    align-items: center;
    column-gap: 12px;
    padding: 12px 16px;
    /* hover/焦点背景不超出外卡圆角 */
    border-radius: 8px;
    user-select: none;
}

.expander-header:hover {
    background-color: var(--control-background-hover);
}

.expander-header:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: -2px;
}

.expander-icon {
    flex-shrink: 0;
    display: flex;
    align-items: center;
}

.expander-main {
    /* 吃掉多余空间，把常显控件区和 chevron 推到最右 */
    flex: 1 1 auto;
    min-width: 0;
}

.expander-header-text {
    font-size: var(--font-size);
}

.expander-description {
    margin-top: 2px;
    font-size: 12px;
    /* 弱化说明文字：两主题下统一用透明度衰减，不引入新颜色 token */
    opacity: 0.6;
}

.expander-actions {
    display: flex;
    align-items: center;
    column-gap: 8px;
}

.expander-chevron {
    flex-shrink: 0;
    width: 12px;
    height: 12px;
    color: var(--control-text-color-active);
    /* 收起时箭头朝右，展开时转回朝下 */
    transform: rotate(-90deg);
    transition: transform 0.2s ease;
}

.fluent-setting-expander.expanded .expander-chevron {
    transform: rotate(0deg);
}

/* 展开区：高度 auto 无法直接 transition，用 grid-template-rows 0fr→1fr 做可动画的高度过渡 */
.expander-content {
    display: grid;
    grid-template-rows: 0fr;
    transition: grid-template-rows 0.2s ease;
}

.fluent-setting-expander.expanded .expander-content {
    grid-template-rows: 1fr;
}

.expander-content-inner {
    /* 0fr 时允许内容被压缩到 0 并裁掉溢出部分 */
    min-height: 0;
    overflow: hidden;
}

.expander-divider {
    height: 1px;
    background-color: var(--border-color);
}

.expander-body {
    display: flex;
    flex-direction: column;
    /* 内嵌卡片间距 4px，与页面卡片列表一致 */
    gap: 4px;
    padding: 4px;
}
</style>
