<script setup lang="ts">
import { computed, onActivated, onBeforeMount, ref } from 'vue';
import { ElDialog, ElMessage, ElPopconfirm } from 'element-plus';
import MarkdownIt from 'markdown-it';
import 'github-markdown-css';
import '../../assets/markdown-dark.css';

import * as api from '../../tauri-api';
import * as globals from '../../logics/globals';
import * as cfg from '../../logics/config';
import * as anki from '../../logics/anki';
import { FluentButton, FluentHyperlink, FluentSettingCard } from '../../fluent-controls';
import OpenFilledSvg from '../../assets/OpenFilled.svg';
import GitHubSvg from '../../assets/github.svg';
import { useHighlight } from './useHighlight';

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();

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
/** 是否正在检查应用更新 */
const checkingAppUpdate = ref(false);

async function handleCheckUpdateClick() {
    try {
        checkingAppUpdate.value = true;
        // 手动检查：force = true 绕过时间间隔检查，但仍使用 ETag 条件请求
        await globals.fetchAndSetLatestAppInfo(true);
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '检查更新失败', kind: 'error' });
        return;
    } finally {
        checkingAppUpdate.value = false;
    }
    if (globals.appUpdateAvailable.value) {
        ElMessage.success(`发现应用新版本：${globals.latestAppVersion.value}`);
    } else {
        ElMessage.success('当前应用已是最新版本');
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
        ElMessage.error(error instanceof Error ? error.message : String(error));
    } finally {
        launchingAnki.value = false;
    }
}
/** 是否打开笔记模板更新说明对话框 */
const templateReleaseNoteDialogVisible = ref(false);
import TEMPLATE_RELEASE_NOTE from '../../assets/model-template-release-note.md?raw';
/** 渲染后的笔记模板更新说明 */
const renderedTemplateReleaseNote = markdownIt.render(TEMPLATE_RELEASE_NOTE);

async function handleUpdateTemplateClick() {
    try {
        await ankiService.updateMarkerModel(effectiveModelName.value);
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '笔记模板更新失败', kind: 'error' });
        return;
    }
    ElMessage.success('笔记模板更新成功');
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
                <FluentButton :accent="true" class="small-button" style="cursor: pointer;"
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
                <ElPopconfirm title="是否更新笔记模板？" confirmButtonText="更新" cancelButtonText="取消" :width="180"
                    @confirm="handleUpdateTemplateClick" v-if="globals.templateUpdateAvailable.value">
                    <template #reference>
                        <FluentButton :accent="true" class="small-button">
                            更新模板
                        </FluentButton>
                    </template>
                </ElPopconfirm>
                <FluentHyperlink v-if="globals.templateUpdateAvailable.value"
                    style="padding: 2px 2px; cursor: default;" title="查看模板更新说明"
                    @click="templateReleaseNoteDialogVisible = true">
                    新版本：{{ anki.CARD_TEMPLATE_VERSION }}
                </FluentHyperlink>
            </FluentSettingCard>
            <FluentSettingCard header="作者" setting-id="author">
                <FluentHyperlink @click="cfg.openInBrowser('https://github.com/zhb2000')"
                    title="https://github.com/zhb2000" style="display: flex; align-items: center; cursor: pointer;">
                    <img src="../../assets/zhb-avatar.png" alt="ZHB"
                        style="width: 28px; height: 28px; margin-right: 8px; border-radius: 50%; border: 1px solid var(--border-bottom-color);">
                    <span>ZHB</span>
                    <OpenFilledSvg style="width: 16px; height: 16px; margin-left: 4px;" />
                </FluentHyperlink>
            </FluentSettingCard>
            <FluentSettingCard header="项目地址" setting-id="project-url">
                <FluentHyperlink @click="cfg.openInBrowser('https://github.com/zhb2000/anki-marker')"
                    title="https://github.com/zhb2000/anki-marker"
                    style="display: flex; align-items: center; cursor: pointer;">
                    <GitHubSvg style="width: 20px; height: 20px; margin-right: 8px;" />
                    <span>zhb2000/anki-marker</span>
                    <OpenFilledSvg style="width: 16px; height: 16px; margin-left: 4px;" />
                </FluentHyperlink>
            </FluentSettingCard>
        </div>

        <ElDialog v-model="appReleaseNoteDialogVisible"
            :title="`Anki 划词助手 ${globals.latestAppVersion.value} 更新说明`" width="80%" center
            class="release-note-dialog">
            <div style="padding: 0px 16px 0px 16px;" class="markdown-body" v-html="renderedAppReleaseNote"></div>
            <template #footer>
                <div style="display: flex; align-items: center; justify-content: center;">
                    <FluentButton :accent="true" class="small-button" style="cursor: pointer;"
                        v-if="globals.appUpdateAvailable.value"
                        @click="cfg.openInBrowser(globals.latestAppHtmlURL.value)"
                        :title="globals.latestAppHtmlURL.value">
                        <span style="display: flex; align-items: center;">
                            <OpenFilledSvg style="width: 16px; height: 16px; margin-right: 4px;" />
                            <span style="padding-bottom: 2px;">下载更新</span>
                        </span>
                    </FluentButton>
                    <FluentButton @click="appReleaseNoteDialogVisible = false" class="small-button">
                        关闭
                    </FluentButton>
                </div>
            </template>
        </ElDialog>
        <ElDialog v-model="templateReleaseNoteDialogVisible"
            :title="`划词助手单词笔记模板 ${anki.CARD_TEMPLATE_VERSION} 更新说明`" width="80%" center
            class="release-note-dialog">
            <div style="padding: 0px 16px 0px 16px;" class="markdown-body" v-html="renderedTemplateReleaseNote">
            </div>
            <template #footer>
                <div style="display: flex; align-items: center; justify-content: center;">
                    <ElPopconfirm title="是否更新笔记模板？" confirmButtonText="更新" cancelButtonText="取消" :width="180"
                        @confirm="handleUpdateTemplateClick">
                        <template #reference>
                            <FluentButton :accent="true" class="small-button">
                                更新模板
                            </FluentButton>
                        </template>
                    </ElPopconfirm>
                    <FluentButton @click="templateReleaseNoteDialogVisible = false" class="small-button">
                        关闭
                    </FluentButton>
                </div>
            </template>
        </ElDialog>
    </div>
</template>

<style>
/* 更新说明对话框的样式：ElDialog 渲染到 body 下，scoped 无法命中，必须保留为非 scoped 全局样式 */
.release-note-dialog .el-dialog__body {
    height: calc(80vh - 150px);
    overflow: auto;
    user-select: text;
    cursor: text;
}

.release-note-dialog {
    user-select: none;
    cursor: default;
}
</style>

<style scoped>
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
