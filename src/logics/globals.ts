import { reactive, ref, watch, computed } from 'vue';
import * as api from '../tauri-api';
import { fetch } from '@tauri-apps/plugin-http';
import * as semver from 'semver';

import { Config } from './config';
import * as anki from './anki';
import { AnkiService } from './anki';
import * as utils from './utils';
import { typeAssertion } from './typing';

/**
 * 禁用应用启动时的应用版本检查
 * （GitHub Release API 有请求频率限制，开发模式下不要频繁请求）
 */
export let DEBUG_DISABLE_ONSTART_APP_CHECK: boolean;
/** 模拟当前应用版本过低 */
export let DEBUG_CURRENT_LOW_APP_VERSION: boolean;
/**
 * 不实际请求 GitHub Release API
 * （GitHub Release API 有请求频率限制，开发模式下不要频繁请求）
 */
export let DEBUG_NOT_REAL_APP_CHECK: boolean;
/** 模拟当前模板版本过低 */
export let DEBUG_CURRENT_LOW_TEMPLATE_VERSION: boolean;

async function initDebugFlags() {
    const IN_DEV_MODE = !await utils.rustInRelease();
    if (IN_DEV_MODE) {
        DEBUG_DISABLE_ONSTART_APP_CHECK = true;
        // DEBUG_CURRENT_LOW_APP_VERSION = true;
        // DEBUG_NOT_REAL_APP_CHECK = true;
        // DEBUG_CURRENT_LOW_TEMPLATE_VERSION = true;
    } else {
        DEBUG_DISABLE_ONSTART_APP_CHECK = false;
        DEBUG_CURRENT_LOW_APP_VERSION = false;
        DEBUG_NOT_REAL_APP_CHECK = false;
        DEBUG_CURRENT_LOW_TEMPLATE_VERSION = false;
    }
}

// #region Config
let config: Config;

async function initConfig() {
    if (config != null) {
        return;
    }
    const rawConfig = await Config.load();
    config = reactive(rawConfig) as Config; // 转化为响应式对象以便监听 config 的变化
}

export async function getConfig(): Promise<Config> {
    await initConfig();
    return config;
}
// #endregion

// #region AnkiService
let ankiService: AnkiService;

async function initAnkiService() {
    if (ankiService != null) {
        return;
    }
    const cfg = await getConfig();
    ankiService = new AnkiService(cfg.ankiConnectURL);
    /** 监听 config 的 anki-connect-url 更新，并同步到 ankiService */
    watch(
        () => config!.ankiConnectURL,
        newURL => ankiService!.url = newURL
    );
}

export async function getAnkiService(): Promise<AnkiService> {
    await initAnkiService();
    return ankiService;
}
// #endregion

// #region app version
interface LatestAppInfo {
    version: string;
    tagName: string;
    htmlURL: string;
    name: string;
    body: string;
}

/** GitHub Release 上最新的应用版本信息 */
export const latestAppInfo = ref<LatestAppInfo | null>(null);
/** GitHub Release 上最新的应用版本（semver 格式） */
export const latestAppVersion = computed(() => latestAppInfo.value?.version);
/** 最新应用版本的 Release 页面 URL */
export const latestAppHtmlURL = computed(() => latestAppInfo.value?.htmlURL);
/** 最新应用版本的 Release 名称 */
export const latestAppName = computed(() => latestAppInfo.value?.name);
/** 最新应用版本的 Release 说明 */
export const latestAppBody = computed(() => latestAppInfo.value?.body);
/** 上一次成功请求的时间戳 */
let lastFetchTimestamp: number | null = null;
/** 当前应用的版本 */
let appVersion: string;
/** 是否有可用的应用更新 */
export const appUpdateAvailable = computed(() => {
    if (latestAppVersion.value == null) {
        return false;
    }
    return semver.gt(latestAppVersion.value, appVersion);
});

/** 是否距离上次成功请求超过 1 分钟 */
function farFromLastAppInfoFetch(): boolean {
    const now = Date.now(); // 当前时间戳（毫秒）
    if (lastFetchTimestamp == null) {
        // 如果从未发送过请求，则需要发送
        return true;
    }
    // 计算距离上次请求的时间差（毫秒），并判断是否超过 1 分钟
    const timeSinceLastFetch = now - lastFetchTimestamp;
    return timeSinceLastFetch > 60 * 1000; // 1 分钟 = 60 * 1000 毫秒F
}

export async function fetchAndSetLatestAppInfo() {
    if (latestAppInfo.value == null || farFromLastAppInfoFetch()) {
        latestAppInfo.value = await getLatestAppInfoFromGitHubRelease();
        lastFetchTimestamp = Date.now();
    }
}

async function makeUserAgent(): Promise<string> {
    const [appVersion, osType] = await Promise.all([
        api.app.getVersion(),
        api.os.type()
    ]);
    return `$Anki-Marker/${appVersion} (${osType}; Tauri)`;
}

async function getLatestAppInfoFromGitHubRelease(): Promise<LatestAppInfo> {
    if (DEBUG_NOT_REAL_APP_CHECK) {
        await new Promise(resolve => setTimeout(resolve, 1000));
        return {
            version: '0.0.1',
            tagName: 'v0.0.1',
            htmlURL: 'https://github.com/zhb2000/anki-marker/releases/tag/v0.0.1',
            name: 'Anki Marker v0.0.1',
            body: '## [0.0.1] - 2024-03-17\r\n第一个版本。\n\n开发测试显示效果用，' +
                '将 `src/logics/globals.ts` 中的 `DEBUG_APP_UPDATE_NOT_FETCH` 设置为 `false` 后可正常获取最新版本。'
        };
    }
    const GITHUB_RELEASE_API = 'https://api.github.com/repos/zhb2000/anki-marker/releases/latest';
    const response = await fetch(GITHUB_RELEASE_API, {
        method: 'GET',
        headers: {
            'Accept': 'application/vnd.github.v3+json', // 推荐明确声明 GitHub API 版本（GitHub API v3）
            'User-Agent': await makeUserAgent() // GitHub REST API 要求提供 User-Agent
        }
    });
    if (!response.ok) {
        throw new Error(
            'HTTP request is not ok. ' +
            `status: ${response.status}, ` +
            `data: ${await response.text()}, ` +
            `headers: ${JSON.stringify(response.headers)}.`
        );
    }
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    const data = await response.json();
    if (!(data instanceof Object)) {
        throw TypeError(`Expect response data to be Object but receive ${typeof data}: ${String(data)}`);
    }
    typeAssertion<{ tag_name: string, html_url: string, name: string, body: string; }>(data);
    const version = semver.clean(data.tag_name);
    if (version == null) {
        throw new Error(`version ${version} cannot be cleaned by semver`);
    }
    return {
        version,
        tagName: data.tag_name,
        htmlURL: data.html_url,
        name: data.name,
        body: data.body
    };
}

async function initAppVersion() {
    if (appVersion == null) {
        appVersion = await api.app.getVersion();
        if (DEBUG_CURRENT_LOW_APP_VERSION) {
            appVersion = '0.0.0';
        }
    }
}

export async function getAppVersion(): Promise<string> {
    await initAppVersion();
    return appVersion;
}
// #endregion

// #region template version
/**
 * Anki 中当前的笔记模板版本
 * - string: 模板版本号
 * - null: 未获取到模板版本
 * - Error: 获取模板版本时出错
 */
export const templateVersion = ref<string | null | Error>(new Error('initializing'));
/** 是否有可用的模板更新 */
export const templateUpdateAvailable = computed(() => {
    if (templateVersion.value == null) {
        return true; // “未知”状态下默认为可更新
    } else if (typeof templateVersion.value === 'string') {
        try {
            return semver.gt(anki.CARD_TEMPLATE_VERSION, templateVersion.value);
        } catch (error) {
            console.error('Error comparing template versions.\n', error);
            // 如果比较版本时出错，认为是可更新
            return true;
        }
    }
    // 获取模板版本时出错，认为是不可更新
    return false;
});

/** 获取 Anki 中的笔记模板版本，出错时不抛出异常，而是将异常信息存入 templateVersion */
export async function fetchAndSetTemplateVersion(modelName: string) {
    if (DEBUG_CURRENT_LOW_TEMPLATE_VERSION) {
        templateVersion.value = '0.0.0';
        return;
    }
    try {
        templateVersion.value = await ankiService.getCardTemplateVersionByModelName(modelName);
    } catch (error) {
        console.error(error);
        templateVersion.value = (error instanceof Error) ? error : new Error(String(error));
    }
}
// #endregion

// #region Theme
/**
 * 设置 element-plus 主题色
 * 
 * - 修改主题色：https://github.com/element-plus/element-plus/discussions/14659
 * - 主题：https://element-plus.org/zh-CN/guide/theming.html
 */
function setElementTheme() {
    const root: HTMLElement = document.documentElement;
    const styles = getComputedStyle(root);
    const accentColor = styles.getPropertyValue('--accent').trim();
    setThemeColor(accentColor);
}

/** 调整亮度生成 element-plus 主题色的 light 颜色 */
function adjustLightness(h: number, s: number, l: number, adjustment: number): string {
    return `hsl(${h}, ${s}%, ${Math.min(l + adjustment, 100)}%)`;
}

function setThemeColor(primaryColor: string): void {
    const root: HTMLElement = document.documentElement;
    // 获取 HSL 值
    const { r, g, b } = utils.hexToRgb(primaryColor);
    const { h, s, l } = utils.rgbToHsl(r, g, b);
    // 设置主色和其他 light 颜色
    root.style.setProperty('--el-color-primary', `hsl(${h}, ${s}%, ${l}%)`);
    root.style.setProperty('--el-color-primary-light-3', adjustLightness(h, s, l, 10));
    root.style.setProperty('--el-color-primary-light-5', adjustLightness(h, s, l, 20));
    root.style.setProperty('--el-color-primary-light-7', adjustLightness(h, s, l, 30));
    root.style.setProperty('--el-color-primary-light-8', adjustLightness(h, s, l, 35));
    root.style.setProperty('--el-color-primary-light-9', adjustLightness(h, s, l, 40));
    root.style.setProperty('--el-color-white', '#ffffff');
    root.style.setProperty('--el-color-black', '#000000');
}
// #endregion

/** 是否已经初始化过 */
let initializedAtAppStart = false;

export async function initAtAppStart() {
    if (initializedAtAppStart) {
        return;
    }
    // 初始化调试标志
    await initDebugFlags();
    // 禁用 WebView 右键菜单和快捷键
    if (await utils.rustInRelease()) {
        utils.disableWebviewContextMenu();
        utils.disableWebviewKeyboardShortcuts();
    }
    // 设置 element-plus 主题色
    setElementTheme();
    // 获取应用版本
    await initAppVersion();
    // 加载配置文件
    try {
        await initConfig();
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '配置文件读取失败', kind: 'error' });
        throw error; // 配置文件读取失败时不继续后续操作
    }
    // 初始化 AnkiService 对象
    await initAnkiService();
    // 启动配置文件监听器
    try {
        await config.startWatcher();
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '配置文件监听失败', kind: 'error' });
        // 配置文件监听失败时仅弹窗报错，不阻止后续操作
    }
    // 获取笔记模板版本，不等待结果。
    // 当模板名称或 AnkiConnect URL 改变时，重新获取模板版本。
    watch(
        // Use a getter function as the watch source to watch properties of a reactive object.
        // See https://vuejs.org/guide/essentials/watchers.html#watch-source-types
        () => [config.modelName, config.ankiConnectURL],
        async ([newModelName]) => await fetchAndSetTemplateVersion(newModelName),
        { immediate: true }
    );
    // 检查应用更新，在初始化代码中不等待更新检查的结果，避免阻塞应用启动
    void (async () => {
        if (DEBUG_DISABLE_ONSTART_APP_CHECK) {
            return;
        }
        try {
            await fetchAndSetLatestAppInfo();
        } catch (error) {
            console.error(error);
            // 应用更新检查失败时仅在控制台报错，不弹窗提示，也不阻止后续操作
        }
    })();
    initializedAtAppStart = true;
}
