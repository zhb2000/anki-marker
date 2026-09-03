<script setup lang="ts">
withDefaults(defineProps<{
    /** 标题 */
    header: string;
    /** 副标题/说明（标题下方，小字弱化）；disabled 时的原因说明也写在这里 */
    description?: string;
    /**
     * 说明文案的语义类型：normal 为常规弱化描述；warning 为警示色（如“API 地址已更换，
     * 请同步更换 Key”这类需要用户动作的提醒），不加弱化以保证可读性
     */
    descriptionType?: 'normal' | 'warning';
    /** 搜索高亮锚点：渲染为根元素的 data-setting-id 属性，供 useHighlight 定位 */
    settingId?: string;
    /** 禁用时整卡弱化（仅视觉弱化，不禁止交互——内部控件的禁用由使用方自行控制） */
    disabled?: boolean;
}>(), {
    description: undefined,
    descriptionType: 'normal',
    settingId: undefined,
    disabled: false,
});
</script>

<template>
    <div class="fluent-setting-card" :class="{ disabled }" :data-setting-id="settingId">
        <!-- 标题左侧图标（可选） -->
        <div v-if="$slots.icon" class="card-icon">
            <slot name="icon"></slot>
        </div>
        <div class="card-main">
            <div class="card-header-row">
                <span class="card-header">{{ header }}</span>
                <!-- 标题右侧小操作区：放 ResetButton 等轻量操作 -->
                <slot name="header-extra"></slot>
            </div>
            <div v-if="description" class="card-description"
                :class="{ 'description-warning': descriptionType === 'warning' }">{{ description }}</div>
        </div>
        <!-- 右侧操作区：卡片的主控件；空间不足时整体换行到下方 -->
        <div v-if="$slots.default" class="card-actions">
            <slot></slot>
        </div>
    </div>
</template>

<style scoped>
.fluent-setting-card {
    display: flex;
    align-items: center;
    /* 窄空间下右侧操作区换行到标题下方（配合 .card-actions 的 margin-left: auto） */
    flex-wrap: wrap;
    column-gap: 12px;
    row-gap: 4px;
    padding: 12px 16px;
    font-family: var(--font-family);
    color: var(--control-text-color);
    background-color: var(--control-background);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    /* 卡片自身不设外边距，卡片间距由父容器 gap 控制 */
    /* 文本不可选中；操作区内输入框等控件的 user-select 由控件自身覆盖 */
    user-select: none;
}

/* 禁用仅做视觉弱化，不禁 pointer-events（内部控件是否可交互由使用方控制） */
.fluent-setting-card.disabled {
    opacity: 0.6;
}

.card-icon {
    flex-shrink: 0;
    display: flex;
    align-items: center;
}

.card-main {
    /* 吃掉多余空间把操作区推到最右；min-width: 0 允许文本区收缩换行 */
    flex: 1 1 auto;
    min-width: 0;
}

.card-header-row {
    display: flex;
    align-items: center;
    column-gap: 8px;
}

.card-header {
    font-size: var(--font-size);
}

.card-description {
    margin-top: 2px;
    font-size: 12px;
    /* 弱化说明文字：两主题下统一用透明度衰减，不引入新颜色 token */
    opacity: 0.6;
}

/* 警示说明：语义色 + 不弱化（须排在 .card-description 之后以覆盖其 opacity）。
   配色复用必填提示的 --warning-text-color（WinUI SystemFillColorWarning 两主题近似值） */
.card-description.description-warning {
    opacity: 1;
    color: var(--warning-text-color);
}

.card-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    column-gap: 8px;
    row-gap: 4px;
    /* 推到卡片最右侧；flex-wrap 换行后仍靠右 */
    margin-left: auto;
}
</style>

<style>
/* 全局样式（非 scoped）：搜索跳转的高亮脉冲 class 由 useHighlight 在运行时动态添加到
   卡片/Expander 根元素上，scoped 属性选择器无法命中这种动态 class，必须全局注册 */
.setting-highlight-flash {
    animation: setting-highlight-flash 1.5s ease-out;
}

@keyframes setting-highlight-flash {
    0% {
        /* 起始色：accent 以 30% 比例混入卡片底色。浅色下 --accent=#0067c0、卡片底 #fbfbfb，
           混合后是明显但不刺眼的浅蓝；深色下 --accent 切换为更亮的 #0078d4、卡片底为
           半透明白（#ffffff0f），在 #202020 窗口底上同样清晰。两主题共用 30%，无需分主题覆写 */
        background-color: color-mix(in srgb, var(--accent) 30%, var(--control-background));
    }
    100% {
        /* 终点与卡片静止背景完全一致（而非 transparent），动画结束/移除 class 时无跳变 */
        background-color: var(--control-background);
    }
}
</style>
