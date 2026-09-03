<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';
import { FluentDialogShell, FluentHyperlink } from '../fluent-controls';
import { openInBrowser } from '../logics/config';
import { LLM_PROVIDER_PRESETS, type LlmProviderPreset } from '../logics/llm-presets';

/**
 * 预设服务商选择弹窗（WinUI ContentDialog 风格，外壳由 FluentDialogShell 提供）。
 *
 * 语义是“帮用户填充”：点击服务商条目后由父组件把其 baseUrl 填进 API 地址输入框
 * （输入框仍是唯一事实来源），本弹窗不持有任何选择状态。
 * 每行展示品牌图标、名称、官方地址与“获取 Key”外链（本地服务为注意事项说明），
 * 底部为使用中转站/自建服务的手动填写提示。
 */

const props = defineProps<{
    /** 弹窗是否可见（挂载/卸载由该值驱动） */
    open: boolean;
}>();

const emit = defineEmits<{
    close: [];
    /** 点击某个服务商条目 */
    select: [preset: LlmProviderPreset];
}>();

// 打开时聚焦第一个条目，支持 Tab/Enter 键盘选择（Esc/遮罩关闭由外壳负责）
const listRef = ref<HTMLElement | null>(null);

watch(() => props.open, open => {
    if (open) {
        void nextTick(() => listRef.value?.querySelector<HTMLButtonElement>('.preset-main')?.focus());
    }
});

/** “获取 Key”外链：阻止冒泡以免触发行选中，新开系统浏览器 */
function onGetKey(event: MouseEvent, preset: LlmProviderPreset): void {
    event.stopPropagation();
    void openInBrowser(preset.homepage);
}
</script>

<template>
    <FluentDialogShell :open="open" title="预设服务商" :commands="[{ key: 'close', label: '关闭' }]"
        @close="emit('close')" @command="emit('close')">
        <div class="dialog-hint">
            选择服务商后会把其官方 API 地址填充到输入框（覆盖现有内容），填充后仍可手动修改。
        </div>
        <ul ref="listRef" class="preset-list">
            <li v-for="preset in LLM_PROVIDER_PRESETS" :key="preset.id">
                <div class="preset-item">
                    <button type="button" class="preset-main" :title="preset.baseUrl"
                        @click="emit('select', preset)">
                        <img class="preset-logo" :class="{ 'invert-in-dark': preset.invertInDark }"
                            :src="preset.logo" alt="" />
                        <span class="preset-text">
                            <span class="preset-name">{{ preset.name }}</span>
                            <span class="preset-url">{{ preset.baseUrl }}</span>
                            <span v-if="preset.note" class="preset-note">{{ preset.note }}</span>
                        </span>
                    </button>
                    <FluentHyperlink v-if="preset.homepage" class="preset-link"
                        :title="`前往 ${preset.name} 开放平台`" @click="onGetKey($event, preset)">
                        获取 Key
                    </FluentHyperlink>
                </div>
            </li>
        </ul>
        <div class="dialog-footer-hint">
            使用第三方中转站或自建服务？直接关闭本窗口，在输入框中手动填写 API 地址即可。
        </div>
    </FluentDialogShell>
</template>

<style scoped>
.dialog-hint {
    flex-shrink: 0;
    padding-bottom: 8px;
    font-size: 12px;
    opacity: 0.6;
    user-select: none;
}

.preset-list {
    flex: 1 1 auto;
    min-height: 0;
    margin: 0;
    padding: 0;
    list-style: none;
    overflow-y: auto;
}

/* 行容器：整行 hover 反馈；左侧主按钮负责选中，右侧外链独立可点 */
.preset-item {
    display: flex;
    align-items: center;
    margin-bottom: 2px;
    border-radius: 4px;
}

.preset-item:hover {
    background-color: var(--flyout-item-background-hover);
}

.preset-item:active {
    background-color: var(--flyout-item-background-active);
}

.preset-main {
    display: flex;
    align-items: center;
    flex: 1 1 auto;
    min-width: 0;
    padding: 6px 8px 6px 10px;
    border: none;
    background-color: transparent;
    color: inherit;
    font-family: var(--font-family);
    text-align: left;
    cursor: pointer;
}

.preset-main:focus-visible {
    outline: 2px solid var(--focus-stroke);
    outline-offset: -2px;
}

.preset-logo {
    flex-shrink: 0;
    width: 20px;
    height: 20px;
    margin-right: 10px;
    border-radius: 3px;
    object-fit: contain;
}

/* 品牌色近黑的服务商图标在深色主题下反色显示 */
html.dark .invert-in-dark {
    filter: invert(1);
}

.preset-text {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-width: 0;
}

.preset-name {
    overflow: hidden;
    font-size: 14px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

/* URL 是要填充的内容本体，换行完整展示（无自然断点，需按字符断行）；
   名称短保持单行省略即可 */
.preset-url {
    font-size: 12px;
    opacity: 0.55;
    word-break: break-all;
}

.preset-note {
    font-size: 12px;
    opacity: 0.55;
}

.preset-link {
    flex-shrink: 0;
    margin-right: 10px;
    padding: 2px 4px;
    font-size: 12px;
    cursor: pointer;
}

.dialog-footer-hint {
    flex-shrink: 0;
    padding: 8px 0 12px;
    font-size: 12px;
    opacity: 0.55;
    user-select: none;
}
</style>
