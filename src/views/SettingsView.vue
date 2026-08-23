<script setup lang="ts">
import { ref, computed, onActivated, onBeforeMount } from 'vue';
import { useRouter } from 'vue-router';
import * as api from '../tauri-api';
import { ElMessage, ElPopconfirm, ElDialog, ElSwitch } from 'element-plus';
import MarkdownIt from 'markdown-it';
import 'github-markdown-css';
import '../assets/markdown-dark.css';

import * as globals from '../logics/globals';
import * as cfg from '../logics/config';
import * as anki from '../logics/anki';
import { FluentInput, FluentButton, FluentHyperlink } from '../fluent-controls';
import { ReturnButton, ResetButton, ShortcutRecorder } from '../components';
import { formatShortcut } from '../logics/shortcut';
import { invoke } from '../logics/utils';
import OpenFilledSvg from '../assets/OpenFilled.svg';
import GitHubSvg from '../assets/github.svg';


/** avoid rendering before the config is loaded */
const pageInitialized = ref(false);
let config: cfg.Config;
let ankiService: anki.AnkiService;
const markdownIt = new MarkdownIt();

// #region 设置项的保存
const router = useRouter();

async function commitConfig() {
    if (config != null) {
        try {
            await config.commit();
        } catch (error) {
            console.error(error);
            await api.dialog.message(String(error), { title: '配置文件保存失败', kind: 'error' });
        }
    }
}

/** 点击返回按钮时先保存设置再返回 */
async function handleReturnClick() {
    await commitConfig();
    router.back();
}

/** 输入框失去焦点时保存设置 */
async function handleInputBlur() {
    await commitConfig();
}

/** 点击输入框右侧的重置按钮时重置设置并保存 */
async function handleResetClick<K extends keyof typeof cfg.CONFIG_DEFAULTS>(key: K) {
    // 配置项的值类型为 string 或 boolean（见 CONFIG_DEFAULTS），断言到具体键的属性类型
    config[key] = cfg.CONFIG_DEFAULTS[key] as cfg.Config[K];
    await commitConfig();
}
// #endregion

// #region 更新应用
let appVersion: string;
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
/** 是否打开笔记模板更新说明对话框 */
const templateReleaseNoteDialogVisible = ref(false);
import TEMPLATE_RELEASE_NOTE from '../assets/model-template-release-note.md?raw';
/** 渲染后的笔记模板更新说明 */
const renderedTemplateReleaseNote = markdownIt.render(TEMPLATE_RELEASE_NOTE);

async function handleUpdateTemplateClick() {
    try {
        await ankiService.updateMarkerModel(config.modelName);
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '笔记模板更新失败', kind: 'error' });
        return;
    }
    ElMessage.success('笔记模板更新成功');
    templateReleaseNoteDialogVisible.value = false;
    await globals.fetchAndSetTemplateVersion(config.modelName); // 刷新笔记模板版本
}
// #endregion

// #region 打开配置文件
/** 点击打开配置文件按钮 */
async function handleOpenFileClick() {
    try {
        await cfg.openFile(config.path);
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '打开文件失败', kind: 'error' });
    }
}

/** 点击打开配置文件所在目录按钮 */
async function handleShowInExplorerClick() {
    try {
        await cfg.showInExplorer(config.path);
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '打开目录失败', kind: 'error' });
    }
}
// #endregion

// #region 全局快捷键
/** 是否为 macOS（全局快捷键目前仅支持 macOS） */
const isMacOS = computed(() => api.os.type() === 'macos');

/**
 * 快捷键设置的响应式镜像。
 * config 对象本身不是响应式的，直接依赖其属性无法驱动界面更新，
 * 故在页面初始化与快捷键变化时同步此 ref。
 */
const globalShortcut = ref('');

/** 辅助功能权限状态：null 表示尚未检查 */
const accessibilityTrusted = ref<boolean | null>(null);

/** 辅助功能权限状态显示文本 */
const accessibilityStatusText = computed(() => {
    if (accessibilityTrusted.value === true) {
        return '✓ 已授权';
    } else if (accessibilityTrusted.value === false) {
        return '⚠️ 未授权';
    }
    return '检查中…';
});

/** 辅助功能权限状态文本的颜色（跟随浅色/深色主题的语义色变量） */
const accessibilityStatusClass = computed(() => {
    if (accessibilityTrusted.value === true) {
        return 'status-success';
    } else if (accessibilityTrusted.value === false) {
        return 'status-warning';
    }
    return '';
});

/** 检查辅助功能权限状态 */
async function checkAccessibilityTrust() {
    if (!isMacOS.value) {
        return;
    }
    try {
        accessibilityTrusted.value = await invoke<boolean>('is_accessibility_trusted');
    } catch (error) {
        console.error(error);
    }
}

/** 点击申请权限按钮：弹出系统授权弹窗；若弹窗曾被拒绝则直接打开系统设置的辅助功能面板 */
async function handleRequestAccessibilityClick() {
    try {
        await invoke('request_accessibility_trust');
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '申请辅助功能权限失败', kind: 'error' });
        return;
    }
    // 授权完成后切回本应用时，由窗口焦点监听自动刷新状态
}

/** 快捷键变更（录制/清除）后保存配置；快捷键从无到有时权限状态行首次出现，顺带检查一次权限 */
async function handleShortcutChange() {
    // v-model 的赋值先于本 handler 执行，此处 config.globalShortcut 已是最新值
    globalShortcut.value = config.globalShortcut;
    await commitConfig();
    void checkAccessibilityTrust();
}

/** 重置快捷键设置后同步响应式镜像 */
async function handleShortcutResetClick() {
    await handleResetClick('globalShortcut');
    globalShortcut.value = config.globalShortcut;
}

/** 快捷键注册结果（Rust 侧在注册/注销后 emit） */
interface ShortcutRegistration {
    shortcut: string;
    success: boolean;
    error: string | null;
}

/** 监听快捷键注册结果事件，用于向用户反馈注册成功或失败（如快捷键冲突） */
async function listenShortcutRegistration() {
    try {
        await api.event.listen<ShortcutRegistration>('shortcut-registration', event => {
            const { shortcut, success, error } = event.payload;
            if (success) {
                ElMessage.success(shortcut.length > 0
                    ? `全局快捷键已注册：${formatShortcut(shortcut)}`
                    : '已停用全局快捷键');
            } else {
                ElMessage.error(`全局快捷键注册失败：${error ?? '未知错误'}`);
            }
        });
    } catch (error) {
        console.error(error);
    }
}
// #endregion

// 由于使用了 KeepAlive 不销毁页面，所以 onMounted 只会执行一次
onBeforeMount(async () => {
    // 为需要初始化的变量赋值
    await globals.initAtAppStart();
    // plugin-os 2.3+ 中 type() 已改为同步函数，且其返回值在此处未被使用，故移除
    [config, ankiService, appVersion] = await Promise.all([
        globals.getConfig(),
        globals.getAnkiService(),
        globals.getAppVersion(),
    ]);
    pageInitialized.value = true;
    await listenShortcutRegistration();
    globalShortcut.value = config.globalShortcut;
    // 从其他应用切回本应用时（如从系统设置授权后返回）自动刷新权限状态；
    // 页面随 KeepAlive 常驻，监听器无需注销
    try {
        await api.window.getCurrentWindow().onFocusChanged(({ payload: focused }) => {
            if (focused) {
                void checkAccessibilityTrust();
            }
        });
    } catch (error) {
        console.error(error);
    }
});

onActivated(async () => {
    // 进入设置页面时检查辅助功能权限状态
    void checkAccessibilityTrust();
    // 打开设置页面时获取/刷新一次笔记模板版本
    // 由于 vue 的生命周期钩子不会等待 async 函数执行完毕，
    // 所以即使 onActivated 在 onBeforeMount 之后执行，页面的 config 变量仍可能未初始化（undefined）
    await globals.fetchAndSetTemplateVersion((await globals.getConfig()).modelName);
});


</script>

<template>
    <div class="main-window">
        <div class="title-bar">
            <ReturnButton style="margin-right: 8px;" @click="handleReturnClick" />
            <h1 style="display: inline-block;">设置</h1>
        </div>
        <div class="content-area" v-if="pageInitialized">
            <h2>应用设置</h2>
            <div class="term">
                <span>AnkiConnect 服务</span>
                <ResetButton @click="handleResetClick('ankiConnectURL')" />
            </div>
            <FluentInput class="input-text" placeholder="请输入 AnkiConnect 服务的 URL" v-model="config.ankiConnectURL"
                @blur="handleInputBlur" />
            <div class="term">
                <span>将划词结果添加到哪个牌组</span>
                <ResetButton @click="handleResetClick('deckName')" />
            </div>
            <FluentInput class="input-text" placeholder="请输入牌组名称" v-model="config.deckName" @blur="handleInputBlur" />
            <div class="term">
                <span>使用的笔记模板名称</span>
                <ResetButton @click="handleResetClick('modelName')" />
            </div>
            <FluentInput class="input-text" placeholder="请输入笔记模板名称" v-model="config.modelName"
                @blur="handleInputBlur" />
            <div class="term">
                <span>自动启动 Anki</span>
                <ResetButton @click="handleResetClick('autoLaunchAnki')" />
            </div>
            <div class="switch-row">
                <ElSwitch v-model="config.autoLaunchAnki" @change="commitConfig" />
                <span class="switch-note">添加笔记时若 Anki 未运行，将自动启动 Anki 并等待其就绪</span>
            </div>
            <div class="term">
                <span>应用启动时启动 Anki</span>
                <ResetButton @click="handleResetClick('launchAnkiOnAppStart')" />
            </div>
            <div class="switch-row">
                <ElSwitch v-model="config.launchAnkiOnAppStart" @change="commitConfig" />
                <span class="switch-note">应用启动时自动启动 Anki，无需等到添加笔记</span>
            </div>
            <div class="term">
                <span>Anki 可执行文件路径</span>
                <ResetButton @click="handleResetClick('ankiExecutablePath')" />
            </div>
            <FluentInput class="input-text" placeholder="留空则自动检测 Anki 路径" v-model="config.ankiExecutablePath"
                @blur="handleInputBlur" />

            <template v-if="isMacOS">
                <div class="term">
                    <span>全局快捷键（录入句子）</span>
                    <ResetButton @click="handleShortcutResetClick" />
                </div>
                <ShortcutRecorder class="input-text" v-model="config.globalShortcut"
                    @update:model-value="handleShortcutChange" />
                <div class="term" v-if="globalShortcut.length > 0">
                    <span style="margin-right: 8px;">辅助功能权限：<span :class="accessibilityStatusClass">{{
                        accessibilityStatusText }}</span></span>
                    <FluentButton v-if="accessibilityTrusted === false" class="update-button"
                        @click="handleRequestAccessibilityClick">申请权限</FluentButton>
                    <FluentButton class="update-button" @click="checkAccessibilityTrust">检查</FluentButton>
                </div>
                <div class="shortcut-note">
                    在任意应用中选中一段文字后按下此快捷键，所选文字将录入划词面板并自动分词。
                </div>
                <div class="shortcut-note" v-if="accessibilityTrusted === false">
                    辅助功能未授权，划词功能可能无法使用。请点击上方“申请权限”按钮，并按系统提示授权本应用。
                </div>
            </template>

            <div style="height: 12px;"></div>
            <h2>关于</h2>
            <div class="term">
                <span style="margin-right: 8px;">应用版本：{{ appVersion }}</span>
                <FluentButton class="update-button" @click="handleCheckUpdateClick" :disabled="checkingAppUpdate">
                    {{ checkingAppUpdate ? '检查中...' : '检查更新' }}
                </FluentButton>
                <FluentButton :accent="true" class="update-button" style="cursor: pointer;"
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
            </div>
            <ElDialog v-model="appReleaseNoteDialogVisible" :title="`Anki 划词助手 ${globals.latestAppVersion.value} 更新说明`"
                width="80%" center class="release-note-dialog">
                <div style="padding: 0px 16px 0px 16px;" class="markdown-body" v-html="renderedAppReleaseNote"></div>
                <template #footer>
                    <div style="display: flex; align-items: center; justify-content: center;">
                        <FluentButton :accent="true" class="update-button" style="cursor: pointer;"
                            v-if="globals.appUpdateAvailable.value"
                            @click="cfg.openInBrowser(globals.latestAppHtmlURL.value)"
                            :title="globals.latestAppHtmlURL.value">
                            <span style="display: flex; align-items: center;">
                                <OpenFilledSvg style="width: 16px; height: 16px; margin-right: 4px;" />
                                <span style="padding-bottom: 2px;">下载更新</span>
                            </span>
                        </FluentButton>
                        <FluentButton @click="appReleaseNoteDialogVisible = false" class="update-button">
                            关闭
                        </FluentButton>
                    </div>
                </template>
            </ElDialog>
            <div class="term">
                <span style="margin-right: 8px;">笔记模板版本：{{ templateVersionDisplay }}</span>
                <FluentButton class="update-button" @click="globals.fetchAndSetTemplateVersion(config.modelName)">
                    刷新
                </FluentButton>
                <ElPopconfirm title="是否更新笔记模板？" confirmButtonText="更新" cancelButtonText="取消" :width="180"
                    @confirm="handleUpdateTemplateClick" v-if="globals.templateUpdateAvailable.value">
                    <template #reference>
                        <FluentButton :accent="true" class="update-button">
                            更新模板
                        </FluentButton>
                    </template>
                </ElPopconfirm>
                <FluentHyperlink v-if="globals.templateUpdateAvailable.value" style="padding: 2px 2px; cursor: default;"
                    title="查看模板更新说明" @click="templateReleaseNoteDialogVisible = true">
                    新版本：{{ anki.CARD_TEMPLATE_VERSION }}
                </FluentHyperlink>
            </div>
            <ElDialog v-model="templateReleaseNoteDialogVisible"
                :title="`划词助手单词笔记模板 ${anki.CARD_TEMPLATE_VERSION} 更新说明`" width="80%" center class="release-note-dialog">
                <div style="padding: 0px 16px 0px 16px;" class="markdown-body" v-html="renderedTemplateReleaseNote">
                </div>
                <template #footer>
                    <div style="display: flex; align-items: center; justify-content: center;">
                        <ElPopconfirm title="是否更新笔记模板？" confirmButtonText="更新" cancelButtonText="取消" :width="180"
                            @confirm="handleUpdateTemplateClick">
                            <template #reference>
                                <FluentButton :accent="true" class="update-button">
                                    更新模板
                                </FluentButton>
                            </template>
                        </ElPopconfirm>
                        <FluentButton @click="templateReleaseNoteDialogVisible = false" class="update-button">
                            关闭
                        </FluentButton>
                    </div>
                </template>
            </ElDialog>
            <div class="term">
                作者：
                <FluentHyperlink @click="cfg.openInBrowser('https://github.com/zhb2000')"
                    title="https://github.com/zhb2000" style="display: flex; align-items: center; cursor: pointer;">
                    <img src="../assets/zhb-avatar.png" alt="ZHB"
                        style="width: 28px; height: 28px; margin-right: 8px; border-radius: 50%; border: 1px solid var(--border-bottom-color);">
                    <span>ZHB</span>
                    <OpenFilledSvg style="width: 16px; height: 16px; margin-left: 4px;" />
                </FluentHyperlink>
            </div>
            <div class="term">
                项目地址：
                <FluentHyperlink @click="cfg.openInBrowser('https://github.com/zhb2000/anki-marker')"
                    title="https://github.com/zhb2000/anki-marker"
                    style="display: flex; align-items: center; cursor: pointer;">
                    <GitHubSvg style="width: 20px; height: 20px; margin-right: 8px;" />
                    <span>zhb2000/anki-marker</span>
                    <OpenFilledSvg style="width: 16px; height: 16px; margin-left: 4px;" />
                </FluentHyperlink>
            </div>

            <div style="height: 24px;"></div>
            <h2>配置文件</h2>
            <div class="term" style="margin-bottom: 16px;">
                <span>
                    <span>安装/便携模式：</span>
                    <span>{{ config.portable ? '便携模式' : '安装模式' }}</span>
                </span>
            </div>
            <div class="term">
                <span>
                    <span>配置文件路径：</span>
                    <span class="file-path">{{ config.path }}</span>
                </span>
            </div>
            <div style="margin-bottom: 0px;">
                <FluentButton class="open-file-button" @click="handleOpenFileClick">打开文件</FluentButton>
                <FluentButton v-if="(['windows', 'macos', 'linux'] as api.os.OsType[]).includes(api.os.type())"
                    class="open-file-button" @click="handleShowInExplorerClick">
                    打开目录
                </FluentButton>
            </div>
        </div>
    </div>
</template>

<style>
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
.main-window {
    /* 不设置 overflow: auto; 会导致顶部出现空白，不知道为什么 */
    overflow: auto;
    background-color: var(--window-background);
    height: 100vh;
    user-select: none;
}

.title-bar {
    top: 0;
    position: sticky;
    /* 提升层级，避免内容区中由 mask-image 等属性创建的层叠上下文（如 ResetButton 图标）穿透到标题栏之上 */
    z-index: 1;
    display: flex;
    align-items: center;
    padding: 32px 20px;
    background-color: var(--window-background);
}

.content-area {
    padding: 32px 24px;
    padding-top: 0;
}

h1 {
    margin: 0;
    padding: 0;
    font-size: 32px;
    font-weight: normal;
    line-height: 32px;
    user-select: none;
    cursor: default;
}

h2 {
    margin: 0;
    margin-bottom: 16px;
    padding: 0;
    font-size: 24px;
    font-weight: normal;
    line-height: 24px;
    user-select: none;
    cursor: default;
}

.term {
    display: flex;
    align-items: center;
    margin-bottom: 8px;
    user-select: none;
    cursor: default;
    font-size: 16px;
}

.input-text {
    height: 32px;
    width: 400px;
    margin-bottom: 24px;
}

.switch-row {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    margin-bottom: 24px;
}

.switch-note {
    font-size: 14px;
    opacity: 0.6;
    user-select: none;
    cursor: default;
}

.shortcut-note {
    font-size: 14px;
    opacity: 0.6;
    max-width: 640px;
    margin-bottom: 8px;
    user-select: none;
    cursor: default;
}

.status-success {
    color: var(--success-text-color);
}

.status-warning {
    color: var(--warning-text-color);
}

.open-file-button {
    margin-right: 8px;
    height: 28px;
    padding-left: 8px;
    padding-right: 8px;
}

.update-button {
    margin-right: 8px;
    height: 28px;
    padding-left: 8px;
    padding-right: 8px;
}

.file-path {
    user-select: text;
    cursor: text;
    overflow-wrap: break-word;
    word-break: break-all;
}
</style>
