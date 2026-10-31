<script setup lang="ts">
import { computed, onActivated, onBeforeMount, ref } from 'vue';
import MarkdownIt from 'markdown-it';
import 'github-markdown-css';
import '../../assets/markdown-dark.css';

import * as globals from '../../logics/globals';
import * as cfg from '../../logics/config';
import * as anki from '../../logics/anki';
import {
    FluentButton,
    FluentDialogShell,
    FluentHyperlink,
    FluentSettingCard,
    FluentStatusText,
    dialog,
    type FluentDialogCommand,
} from '../../fluent-controls';
import { useTransientNotice } from '../../fluent-controls/useTransientNotice';
import OpenFilledSvg from '../../assets/OpenFilled.svg';
import GitHubSvg from '../../assets/github.svg';
import { useHighlight } from './useHighlight';

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();

/**
 * 就地反馈：结果写在触发它的那张设置卡片底部（#notice 插槽），
 * 不再占用窗口顶部的全局信息条。
 * - 应用版本卡片：检查更新的结果
 * - 笔记模板卡片：启动 Anki 失败 / 模板更新成功
 */
const { notice: appVersionNotice, show: showAppVersionNotice } = useTransientNotice(3000);
const { notice: templateNotice, show: showTemplateNotice } = useTransientNotice(3000);

/** avoid rendering before the config is loaded */
const pageInitialized = ref(false);
let config: cfg.Config;
let ankiService: anki.AnkiService;
let appVersion: string;
const markdownIt = new MarkdownIt();

/** 笔记模板名称的生效值（留空表示使用内置默认模板，见 config.ts 的 TEXT_SETTING_FALLBACKS） */
const effectiveModelName = computed(() => cfg.effectiveTextSetting('modelName', config.modelName));

// #region 更新应用
/** 是否打开应用更新说明对话框 */
const appReleaseNoteDialogVisible = ref(false);
/** 渲染后的应用更新说明 */
const renderedAppReleaseNote = computed(() => markdownIt.render(
    `# ${globals.latestAppName.value ?? ''}\n` +
    `${globals.latestAppBody.value ?? ''}`
));
/** 更新说明弹窗的命令区：有更新时给出下载入口（默认按钮），否则只有关闭 */
const appReleaseCommands = computed<FluentDialogCommand[]>(() => {
    if (globals.appUpdateAvailable.value) {
        return [
            { key: 'download', label: '下载更新', accent: true },
            { key: 'close', label: '关闭' },
        ];
    }
    return [{ key: 'close', label: '关闭', accent: true }];
});

/** 更新说明弹窗的命令：下载走浏览器打开更新页，其余（关闭）收起弹窗 */
function onAppReleaseCommand(key: string): void {
    if (key === 'download') {
        void cfg.openInBrowser(globals.latestAppHtmlURL.value);
        return;
    }
    appReleaseNoteDialogVisible.value = false;
}

/** 是否正在检查应用更新 */
const checkingAppUpdate = ref(false);

async function handleCheckUpdateClick() {
    try {
        checkingAppUpdate.value = true;
        // 手动检查：force = true 绕过时间间隔检查，但仍使用 ETag 条件请求
        await globals.fetchAndSetLatestAppInfo(true);
    } catch (error) {
        console.error(error);
        await dialog.message(String(error), { title: '检查更新失败', kind: 'error' });
        return;
    } finally {
        checkingAppUpdate.value = false;
    }
    if (globals.appUpdateAvailable.value) {
        showAppVersionNotice(`发现应用新版本：${globals.latestAppVersion.value}`, { severity: 'success', duration: 5000 });
    } else {
        showAppVersionNotice('当前应用已是最新版本', { severity: 'success' });
    }
}
// #endregion

// #region 更新模板
/** 界面上显示的笔记模板版本 */
const templateVersionDisplay = computed(() => {
    if (globals.templateVersion.value == null) {
        return '未知';
    } else if (globals.templateVersion.value instanceof Error) {
        return '获取失败';
    }
    return globals.templateVersion.value;
});
/** 模板版本获取失败时的错误对象；非失败态为 null */
const templateVersionError = computed(() => {
    const version = globals.templateVersion.value;
    return version instanceof Error ? version : null;
});
/** 模板版本卡的失败原因说明（小字弱化，显示在卡片的 description 位置） */
const templateFailureDescription = computed(() =>
    templateVersionError.value != null ? `获取失败：${templateVersionError.value.message}` : undefined
);
/** 是否正在启动 Anki（点击期间禁用按钮防重复） */
const launchingAnki = ref(false);

/** 启动 Anki：复用 globals 的“探活 → 拉起 → 等待 AnkiConnect 就绪”封装，就绪后自动刷新模板版本 */
async function handleLaunchAnkiClick() {
    launchingAnki.value = true;
    try {
        // 用户显式点击的启动：forceLaunch 无视“自动启动 Anki”设置（该设置只约束添加笔记时的隐式启动）
        await globals.ensureAnkiConnect(undefined, { forceLaunch: true });
        await globals.fetchAndSetTemplateVersion(effectiveModelName.value);
    } catch (error) {
        console.error(error);
        showTemplateNotice(error instanceof Error ? error.message : String(error), {
            severity: 'error',
            duration: 5000,
        });
    } finally {
        launchingAnki.value = false;
    }
}
/** 是否打开笔记模板更新说明对话框 */
const templateReleaseNoteDialogVisible = ref(false);
import TEMPLATE_RELEASE_NOTE from '../../assets/model-template-release-note.md?raw';
/** 渲染后的笔记模板更新说明 */
const renderedTemplateReleaseNote = markdownIt.render(TEMPLATE_RELEASE_NOTE);

/** 模板更新说明弹窗的命令区 */
const templateReleaseCommands: FluentDialogCommand[] = [
    { key: 'update', label: '更新模板', accent: true },
    { key: 'close', label: '关闭' },
];

/**
 * 更新笔记模板前的确认：卡片与更新说明弹窗共用同一入口（原来是一处 ElPopconfirm、
 * 一处弹窗内确认，风格与文案都不统一；统一走命令式 ContentDialog，嵌套时由
 * dialogLayer 保证后打开的弹窗压在上层）
 */
async function confirmUpdateTemplate(): Promise<void> {
    const ok = await dialog.confirm('是否更新笔记模板？', {
        title: '更新笔记模板',
        okLabel: '更新',
        cancelLabel: '取消',
    });
    if (ok) {
        await handleUpdateTemplateClick();
    }
}

/** 模板更新说明弹窗的命令：更新走确认流程，其余（关闭）收起弹窗 */
async function onTemplateReleaseCommand(key: string): Promise<void> {
    if (key !== 'update') {
        templateReleaseNoteDialogVisible.value = false;
        return;
    }
    await confirmUpdateTemplate();
}

async function handleUpdateTemplateClick() {
    try {
        await ankiService.updateMarkerModel(effectiveModelName.value);
    } catch (error) {
        console.error(error);
        await dialog.message(String(error), { title: '笔记模板更新失败', kind: 'error' });
        return;
    }
    showTemplateNotice('笔记模板更新成功', { severity: 'success' });
    templateReleaseNoteDialogVisible.value = false;
    await globals.fetchAndSetTemplateVersion(effectiveModelName.value); // 刷新笔记模板版本
}
// #endregion

onBeforeMount(async () => {
    [config, ankiService, appVersion] = await Promise.all([
        globals.getConfig(),
        globals.getAnkiService(),
        globals.getAppVersion(),
    ]);
    pageInitialized.value = true;
});

onActivated(async () => {
    // 打开设置页面时获取/刷新一次笔记模板版本
    // 由于 vue 的生命周期钩子不会等待 async 函数执行完毕，
    // 所以即使 onActivated 在 onBeforeMount 之后执行，页面的 config 变量仍可能未初始化（undefined）
    await globals.fetchAndSetTemplateVersion(
        cfg.effectiveTextSetting('modelName', (await globals.getConfig()).modelName));
});</script>

<template>
    <div class="settings-page" v-if="pageInitialized">
        <h2 class="group-title">关于</h2>
        <div class="card-list">
            <FluentSettingCard header="应用版本" setting-id="check-update">
                <span class="value-text">{{ appVersion }}</span>
                <FluentButton class="small-button" @click="handleCheckUpdateClick" :disabled="checkingAppUpdate">
                    {{ checkingAppUpdate ? '检查中...' : '检查更新' }}
                </FluentButton>
                <FluentButton :accent="true" class="small-button"
                    v-if="globals.appUpdateAvailable.value" @click="cfg.openInBrowser(globals.latestAppHtmlURL.value)"
                    :title="globals.latestAppHtmlURL.value">
                    <span style="display: flex; align-items: center;">
                        <OpenFilledSvg style="width: 16px; height: 16px; margin-right: 4px;" />
                        <span style="padding-bottom: 2px;">下载更新</span>
                    </span>
                </FluentButton>
                <FluentHyperlink v-if="globals.appUpdateAvailable.value" style="padding: 2px 2px; cursor: default;"
                    title="查看应用更新说明" @click="appReleaseNoteDialogVisible = true">
                    新版本：{{ globals.latestAppVersion }}
                </FluentHyperlink>
                <template #notice>
                    <FluentStatusText :severity="appVersionNotice?.severity ?? 'success'"
                        :message="appVersionNotice?.message ?? null" />
                </template>
            </FluentSettingCard>
            <FluentSettingCard header="Anki 内笔记模板版本" setting-id="update-template"
                :description="templateFailureDescription">
                <span class="value-text">{{ templateVersionDisplay }}</span>
                <FluentButton class="small-button" @click="globals.fetchAndSetTemplateVersion(effectiveModelName)">
                    刷新
                </FluentButton>
                <FluentButton v-if="templateVersionError != null" class="small-button" :disabled="launchingAnki"
                    @click="handleLaunchAnkiClick" accent>
                    启动 Anki
                </FluentButton>
                <FluentButton :accent="true" class="small-button"
                    v-if="globals.templateUpdateAvailable.value" @click="confirmUpdateTemplate">
                    更新模板
                </FluentButton>
                <FluentHyperlink v-if="globals.templateUpdateAvailable.value"
                    style="padding: 2px 2px; cursor: default;" title="查看模板更新说明"
                    @click="templateReleaseNoteDialogVisible = true">
                    新版本：{{ anki.CARD_TEMPLATE_VERSION }}
                </FluentHyperlink>
                <template #notice>
                    <FluentStatusText :severity="templateNotice?.severity ?? 'success'"
                        :message="templateNotice?.message ?? null" />
                </template>
            </FluentSettingCard>
            <FluentSettingCard header="作者" setting-id="author">
                <FluentHyperlink @click="cfg.openInBrowser('https://github.com/zhb2000')"
                    title="https://github.com/zhb2000" style="display: flex; align-items: center;">
                    <img src="../../assets/zhb-avatar.png" alt="ZHB"
                        style="width: 28px; height: 28px; margin-right: 8px; border-radius: 50%; border: 1px solid var(--border-bottom-color);">
                    <span>ZHB</span>
                    <OpenFilledSvg style="width: 16px; height: 16px; margin-left: 4px;" />
                </FluentHyperlink>
            </FluentSettingCard>
            <FluentSettingCard header="项目地址" setting-id="project-url">
                <FluentHyperlink @click="cfg.openInBrowser('https://github.com/zhb2000/anki-marker')"
                    title="https://github.com/zhb2000/anki-marker"
                    style="display: flex; align-items: center;">
                    <GitHubSvg style="width: 20px; height: 20px; margin-right: 8px;" />
                    <span>zhb2000/anki-marker</span>
                    <OpenFilledSvg style="width: 16px; height: 16px; margin-left: 4px;" />
                </FluentHyperlink>
            </FluentSettingCard>
        </div>

        <FluentDialogShell :open="appReleaseNoteDialogVisible"
            :title="`Anki 划词助手 ${globals.latestAppVersion.value} 更新说明`"
            width="80%" max-height="80vh" :commands="appReleaseCommands"
            @close="appReleaseNoteDialogVisible = false" @command="onAppReleaseCommand">
            <div class="markdown-body release-note" v-html="renderedAppReleaseNote"></div>
        </FluentDialogShell>
        <FluentDialogShell :open="templateReleaseNoteDialogVisible"
            :title="`划词助手单词笔记模板 ${anki.CARD_TEMPLATE_VERSION} 更新说明`"
            width="80%" max-height="80vh" :commands="templateReleaseCommands"
            @close="templateReleaseNoteDialogVisible = false" @command="onTemplateReleaseCommand">
            <div class="markdown-body release-note" v-html="renderedTemplateReleaseNote"></div>
        </FluentDialogShell>
    </div>
</template>

<style scoped>
/* 更新说明正文：可选中复制；底色透出对话框内容区（暗色下 github-markdown-css 的
   背景变量已是 transparent，浅色显式覆盖以免出现一块与容器不同的白底） */
.release-note {
    background-color: transparent;
    user-select: text;
    cursor: text;
}

.group-title {
    margin: 24px 0 6px 2px;
    padding: 0;
    font-size: 14px;
    font-weight: normal;
    opacity: 0.6;
    user-select: none;
    cursor: default;
}

.group-title:first-child {
    margin-top: 0;
}

.card-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
}

.value-text {
    user-select: none;
    cursor: default;
}

.small-button {
    height: 28px;
    padding-left: 8px;
    padding-right: 8px;
}
</style>
