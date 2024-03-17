<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import { message as dialogMessage } from '@tauri-apps/api/dialog';
import { type as osType } from '@tauri-apps/api/os';
import type { OsType } from '@tauri-apps/api/os';
import { getVersion } from '@tauri-apps/api/app';

import type { Config } from '../logics/config';
import { CONFIG_DEFAULTS, showInExplorer, openFile, startConfigWatcher } from '../logics/config';
import { getConfig } from '../logics/globals';
import { FluentInput, FluentButton, FluentHyperlink } from '../fluent-controls';
import ReturnButton from '../components/ReturnButton.vue';
import ResetButton from '../components/ResetButton.vue';

const router = useRouter();

let config: Config;
let os: OsType;
let appVersion: string;
/** avoid rendering before the config is loaded */
const showContent = ref(false);

async function commitConfig() {
    if (config != null) {
        try {
            await config.commit();
        } catch (error) {
            console.error(error);
            await dialogMessage(String(error), { title: '配置文件保存失败', type: 'error' });
        }
    }
}

async function handleReturnClick() {
    await commitConfig();
    router.back();
}

async function handleInputBlur() {
    await commitConfig();
}

async function handleResetClick(key: keyof typeof CONFIG_DEFAULTS) {
    config[key] = CONFIG_DEFAULTS[key];
    await commitConfig();
}

async function handleOpenFileClick() {
    try {
        await openFile(config.path);
    } catch (error) {
        console.error(error);
        await dialogMessage(String(error), { title: '打开文件失败', type: 'error' });
    }
}

async function handleShowInExplorerClick() {
    try {
        await showInExplorer(config.path);
    } catch (error) {
        console.error(error);
        await dialogMessage(String(error), { title: '打开目录失败', type: 'error' });
    }
}

onMounted(async () => {
    try {
        await Promise.all([
            getConfig().then(c => config = c),
            osType().then(o => os = o),
            getVersion().then(v => appVersion = v),
        ]);
        showContent.value = true;
    } catch (error) {
        console.error(error);
        await dialogMessage(String(error), { title: '配置文件读取失败', type: 'error' });
        return;
    }
    try {
        await startConfigWatcher(config);
    } catch (error) {
        console.error(error);
        await dialogMessage(String(error), { title: '配置文件监听失败', type: 'error' });
    }
});
</script>

<template>
    <div class="main-window use-fluent-scrollbar">
        <div class="title-bar">
            <ReturnButton style="margin-right: 8px;" @click="handleReturnClick" />
            <h1 style="display: inline-block;">设置</h1>
        </div>
        <div class="content-area" v-if="showContent">
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
            <div style="height: 12px;"></div>
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
            <div style="margin-bottom: 36px;">
                <FluentButton class="open-file-button" @click="handleOpenFileClick">打开文件</FluentButton>
                <FluentButton v-if="os === 'Windows_NT' || os === 'Darwin'" class="open-file-button"
                    @click="handleShowInExplorerClick">打开目录</FluentButton>
            </div>
            <h2>关于</h2>
            <div class="term">应用版本：{{ appVersion }}</div>
            <div class="term">作者：ZHB</div>
            <div class="term">
                <span>
                    项目地址：
                    <FluentHyperlink href="https://github.com/zhb2000/anki-marker" target="_blank">
                        zhb2000/anki-marker
                    </FluentHyperlink>
                </span>
            </div>
        </div>
    </div>
</template>

<style scoped>
.main-window {
    /* 不设置 overflow: auto; 会导致顶部出现空白，不知道为什么 */
    overflow: auto;
    background-color: var(--window-background);
    height: 100vh;
}

.title-bar {
    top: 0;
    position: sticky;
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
}

h2 {
    margin: 0;
    margin-bottom: 16px;
    padding: 0;
    font-size: 24px;
    font-weight: normal;
    line-height: 24px;
    user-select: none;
}

.term {
    display: flex;
    align-items: center;
    margin-bottom: 8px;
    user-select: none;
    font-size: 16px;
}

.input-text {
    height: 32px;
    width: 400px;
    margin-bottom: 24px;
}

.open-file-button {
    margin-right: 8px;
    height: 28px;
    padding-left: 8px;
    padding-right: 8px;
}

.file-path {
    user-select: text;
    overflow-wrap: break-word;
    word-break: break-all;
}
</style>
