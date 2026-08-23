import { reactive, ref, watch, computed } from 'vue';
import * as api from '../tauri-api';
import { fetch } from '@tauri-apps/plugin-http';
import * as semver from 'semver';
import 'element-plus/theme-chalk/dark/css-vars.css';

import { Config } from './config';
import * as anki from './anki';
import { AnkiService } from './anki';
import * as utils from './utils';
import { typeAssertion } from './typing';
import * as preference from './preference';

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
/** 上一次成功请求的时间戳（应用启动时从持久化缓存恢复） */
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

/** localStorage 中持久化的最新应用版本信息缓存 */
interface LatestAppInfoCache {
    /** 上次成功检查的时间戳（毫秒） */
    timestamp: number;
    /** GitHub API 响应的 ETag，用于 If-None-Match 条件请求（304 响应不计入 API 限额） */
    etag: string | null;
    /** 缓存的版本信息 */
    info: LatestAppInfo;
}

/** localStorage 中缓存最新应用版本信息的 key */
const LATEST_APP_INFO_CACHE_KEY = 'latestAppInfoCache';
/** 自动检查更新的最小间隔（毫秒），距上次成功检查未超过此间隔时直接使用缓存、不发请求 */
const AUTO_CHECK_MIN_INTERVAL = 24 * 60 * 60 * 1000; // 24 小时
/** 持久化缓存的内存副本（null 表示尚未从 localStorage 读取过） */
let appInfoCache: LatestAppInfoCache | null = null;

/** 校验 localStorage 中的缓存数据结构是否有效（localStorage 中的数据不可信，需做运行时校验） */
function isValidAppInfoCache(value: unknown): value is LatestAppInfoCache {
    if (value == null || typeof value !== 'object') {
        return false;
    }
    const cache = value as Partial<LatestAppInfoCache>;
    if (typeof cache.timestamp !== 'number' || !Number.isFinite(cache.timestamp)) {
        return false;
    }
    if (cache.etag != null && typeof cache.etag !== 'string') {
        return false;
    }
    const info = cache.info as Partial<LatestAppInfo> | undefined;
    if (info == null || typeof info !== 'object') {
        return false;
    }
    return typeof info.version === 'string' && typeof info.tagName === 'string' &&
        typeof info.htmlURL === 'string' && typeof info.name === 'string' &&
        typeof info.body === 'string';
}

/** 读取持久化的缓存（不存在或无效时返回 null，结果会缓存在内存中） */
function loadAppInfoCache(): LatestAppInfoCache | null {
    if (appInfoCache == null) {
        const value = preference.get(LATEST_APP_INFO_CACHE_KEY);
        if (isValidAppInfoCache(value)) {
            appInfoCache = value;
        }
    }
    return appInfoCache;
}

/** 将缓存保存到 localStorage 并更新内存副本 */
function saveAppInfoCache(cache: LatestAppInfoCache): void {
    appInfoCache = cache;
    preference.set(LATEST_APP_INFO_CACHE_KEY, cache);
}

/**
 * 获取 GitHub Release 上最新的应用版本信息，并更新 `latestAppInfo`。
 *
 * 为避免过度消耗 GitHub API 限额（匿名请求为 60 次/小时/IP）：
 * - `force` 为 `false` 时（应用启动自动检查）：距上次成功检查未超过
 *   `AUTO_CHECK_MIN_INTERVAL` 时直接使用持久化缓存，不发请求
 * - `force` 为 `true` 时（用户手动检查）：跳过时间间隔判断，但仍携带
 *   ETag 条件请求，资源未变化时 GitHub 返回 304 Not Modified（不计入限额）
 *
 * @param force - 是否跳过时间间隔检查（用于用户手动触发检查更新的场景）
 */
export async function fetchAndSetLatestAppInfo(force = false) {
    // 从持久化缓存恢复内存状态（应用刚启动、尚未发起过请求的场景）
    const cache = loadAppInfoCache();
    if (cache != null && lastFetchTimestamp == null) {
        lastFetchTimestamp = cache.timestamp;
        if (latestAppInfo.value == null) {
            latestAppInfo.value = cache.info;
        }
    }

    if (DEBUG_NOT_REAL_APP_CHECK) {
        // mock 分支：不读写持久化缓存，避免调试数据污染真实缓存
        const { info } = await getLatestAppInfoFromGitHubRelease();
        latestAppInfo.value = info;
        lastFetchTimestamp = Date.now();
        return;
    }

    if (!force && lastFetchTimestamp != null &&
        Date.now() - lastFetchTimestamp < AUTO_CHECK_MIN_INTERVAL) {
        return; // 距上次成功检查未超过最小间隔，直接使用缓存
    }

    const { info, etag: newEtag } = await getLatestAppInfoFromGitHubRelease(cache?.etag ?? undefined);
    lastFetchTimestamp = Date.now();
    if (info != null) {
        latestAppInfo.value = info;
    }
    if (latestAppInfo.value != null) {
        saveAppInfoCache({
            timestamp: lastFetchTimestamp,
            etag: newEtag,
            info: latestAppInfo.value
        });
    }
}

async function makeUserAgent(): Promise<string> {
    const appVersion = await api.app.getVersion();
    const osType = api.os.type();
    return `$Anki-Marker/${appVersion} (${osType}; Tauri)`;
}

/** GitHub Release API 的请求结果 */
interface GitHubReleaseResponse {
    /** 版本信息；304 Not Modified 时为 null（资源未变化，应继续使用缓存） */
    info: LatestAppInfo | null;
    /** 响应的 ETag（用于后续的条件请求），响应中不含 ETag 时为 null */
    etag: string | null;
}

async function getLatestAppInfoFromGitHubRelease(etag?: string): Promise<GitHubReleaseResponse> {
    if (DEBUG_NOT_REAL_APP_CHECK) {
        await new Promise(resolve => setTimeout(resolve, 1000));
        return {
            info: {
                version: '0.0.1',
                tagName: 'v0.0.1',
                htmlURL: 'https://github.com/zhb2000/anki-marker/releases/tag/v0.0.1',
                name: 'Anki Marker v0.0.1',
                body: '## [0.0.1] - 2024-03-17\r\n第一个版本。\n\n开发测试显示效果用，' +
                    '将 `src/logics/globals.ts` 中的 `DEBUG_APP_UPDATE_NOT_FETCH` 设置为 `false` 后可正常获取最新版本。'
            },
            etag: null
        };
    }
    const GITHUB_RELEASE_API = 'https://api.github.com/repos/zhb2000/anki-marker/releases/latest';
    const headers: Record<string, string> = {
        'Accept': 'application/vnd.github.v3+json', // 推荐明确声明 GitHub API 版本（GitHub API v3）
        'User-Agent': await makeUserAgent() // GitHub REST API 要求提供 User-Agent
    };
    if (etag != null) {
        // 条件请求：资源未变化时 GitHub 返回 304 Not Modified，且 304 响应不计入 API 限额
        headers['If-None-Match'] = etag;
    }
    const response = await fetch(GITHUB_RELEASE_API, { method: 'GET', headers });
    if (response.status === 304) {
        // 资源未变化，调用方应继续使用缓存中的版本信息
        return { info: null, etag: etag ?? null };
    }
    if (!response.ok) {
        throw new Error(
            'HTTP request is not ok. ' +
            `status: ${response.status}, ` +
            `data: ${await response.text()}, ` +
            `headers: ${JSON.stringify(response.headers)}.`
        );
    }
    const responseEtag = response.headers.get('etag');
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
        info: {
            version,
            tagName: data.tag_name,
            htmlURL: data.html_url,
            name: data.name,
            body: data.body
        },
        etag: responseEtag
    };
}

async function initAppVersion() {
    if (appVersion != null) {
        return;
    }
    appVersion = await api.app.getVersion();
    if (DEBUG_CURRENT_LOW_APP_VERSION) {
        appVersion = '0.0.0';
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
 * - 暗色色阶公式：node_modules/element-plus/theme-chalk/dark/css-vars.css
 *
 * 每次主题切换后都需要调用（`--accent` 的值可能随暗色模式改变，必须重新读取）。
 */
export function setElementTheme() {
    const root: HTMLElement = document.documentElement;
    const isDark = root.classList.contains('dark');
    const styles = getComputedStyle(root);
    const accentColor = styles.getPropertyValue('--accent').trim();
    setThemeColor(accentColor, isDark);
}

/** 调整亮度生成 element-plus 主题色的 light 颜色（浅色模式） */
function adjustLightness(h: number, s: number, l: number, adjustment: number): string {
    return `hsl(${h}, ${s}%, ${Math.min(l + adjustment, 100)}%)`;
}

interface RgbColor {
    r: number;
    g: number;
    b: number;
}

/**
 * 按 element-plus 官方公式混色：mix(mixColor, primary, level)
 * 即 level% 的 mixColor 与 (100 - level)% 的 primary 线性混合
 */
function mixWithPrimary(primary: RgbColor, mixColor: RgbColor, level: number): string {
    const weight = level / 100;
    const r = Math.round((mixColor.r * weight + primary.r * (1 - weight)) * 10) / 10;
    const g = Math.round((mixColor.g * weight + primary.g * (1 - weight)) * 10) / 10;
    const b = Math.round((mixColor.b * weight + primary.b * (1 - weight)) * 10) / 10;
    return `rgb(${r}, ${g}, ${b})`;
}

function setThemeColor(primaryColor: string, isDark: boolean): void {
    const root: HTMLElement = document.documentElement;
    // 获取 HSL 值
    const { r, g, b } = utils.hexToRgb(primaryColor);
    const { h, s, l } = utils.rgbToHsl(r, g, b);
    const primary: RgbColor = { r, g, b };
    // 设置主色
    root.style.setProperty('--el-color-primary', `hsl(${h}, ${s}%, ${l}%)`);
    if (isDark) {
        // 暗色模式：light-N 为 primary 与暗色背景 #141414 按 N*10% 的混色，
        // dark-2 为 primary 与白色按 20% 的混色（官方暗色公式）
        const darkBackground: RgbColor = { r: 0x14, g: 0x14, b: 0x14 };
        const white: RgbColor = { r: 0xff, g: 0xff, b: 0xff };
        root.style.setProperty('--el-color-primary-light-3', mixWithPrimary(primary, darkBackground, 30));
        root.style.setProperty('--el-color-primary-light-5', mixWithPrimary(primary, darkBackground, 50));
        root.style.setProperty('--el-color-primary-light-7', mixWithPrimary(primary, darkBackground, 70));
        root.style.setProperty('--el-color-primary-light-8', mixWithPrimary(primary, darkBackground, 80));
        root.style.setProperty('--el-color-primary-light-9', mixWithPrimary(primary, darkBackground, 90));
        root.style.setProperty('--el-color-primary-dark-2', mixWithPrimary(primary, white, 20));
    } else {
        // 浅色模式：保持原有 HSL 加亮算法，
        // dark-2 为 primary 与黑色按 20% 的混色（官方浅色公式）
        const black: RgbColor = { r: 0, g: 0, b: 0 };
        root.style.setProperty('--el-color-primary-light-3', adjustLightness(h, s, l, 10));
        root.style.setProperty('--el-color-primary-light-5', adjustLightness(h, s, l, 20));
        root.style.setProperty('--el-color-primary-light-7', adjustLightness(h, s, l, 30));
        root.style.setProperty('--el-color-primary-light-8', adjustLightness(h, s, l, 35));
        root.style.setProperty('--el-color-primary-light-9', adjustLightness(h, s, l, 40));
        root.style.setProperty('--el-color-primary-dark-2', mixWithPrimary(primary, black, 20));
    }
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
    // 检查应用更新，在初始化代码中不等待更新检查的结果，避免阻塞应用启动。
    // 距上次成功检查未超过 AUTO_CHECK_MIN_INTERVAL 时会直接使用持久化缓存，不发请求
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
