import { reactive, ref, watch, computed } from 'vue';
import * as api from '../tauri-api';
import { fetch } from '@tauri-apps/plugin-http';
import * as semver from 'semver';

import { Config } from './config';
import * as anki from './anki';
import { AnkiService } from './anki';
import * as utils from './utils';
import { typeAssertion } from './typing';
import * as preference from './preference';
import * as debug from './debug';
import { initShortcutStatus } from './shortcut-status';
import { setElementTheme } from './element-theme';
import { setThemeMode } from './theme';

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

// #region AnkiConnect 可用性保障
/** AnkiConnect 探活请求的超时时间（毫秒） */
const ANKI_CONNECT_PROBE_TIMEOUT_MS = 2000;
/** 启动 Anki 后等待 AnkiConnect 就绪的总超时时间（毫秒） */
const ANKI_LAUNCH_WAIT_TIMEOUT_MS = 60 * 1000;
/** 启动 Anki 后轮询探活的间隔（毫秒） */
const ANKI_LAUNCH_POLL_INTERVAL_MS = 500;
/** ensureAnkiConnect 正在执行中的 Promise，用于防止重入（并发调用共享同一次执行） */
let ensureAnkiConnectInFlight: Promise<void> | null = null;

function sleep(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
}

/** 探活 AnkiConnect 服务（带超时），成功时 resolve，失败时 reject */
async function probeAnkiConnect(): Promise<void> {
    const service = await getAnkiService();
    // 使用 AbortController 而非 AbortSignal.timeout，以兼容不支持后者的 WebView
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), ANKI_CONNECT_PROBE_TIMEOUT_MS);
    try {
        await service.version(controller.signal);
    } finally {
        clearTimeout(timer);
    }
}

async function ensureAnkiConnectImpl(onProgress?: (message: string) => void, forceLaunch = false): Promise<void> {
    // 先探活，AnkiConnect 可用时直接返回
    try {
        await probeAnkiConnect();
        return;
    } catch {
        // AnkiConnect 不可用，继续后续处理
    }
    // Anki 在运行但连不上，多半是未安装或未启用 AnkiConnect 插件
    if (await utils.invoke<boolean>('is_anki_running')) {
        throw new Error(
            'Anki 正在运行，但无法连接 AnkiConnect。' +
            '请确认已安装并启用 AnkiConnect 插件（安装代码 2055492159），并检查 AnkiConnect 服务地址设置。'
        );
    }
    const cfg = await getConfig();
    // “自动启动 Anki”设置只约束隐式启动（如添加笔记时顺带拉起）；
    // 用户显式发起的启动（forceLaunch，如设置页的“启动 Anki”按钮）不受该设置约束
    if (!cfg.autoLaunchAnki && !forceLaunch) {
        throw new Error('无法连接 AnkiConnect，且未启用自动启动 Anki。请手动启动 Anki，或在设置中开启“自动启动 Anki”。');
    }
    // 拉起 Anki，然后串行轮询等待 AnkiConnect 就绪（上一次探活结束后再 sleep，避免请求堆积）
    // 注意：该命令标注了 rename_all = "snake_case"（项目惯例），参数 key 须用 snake_case
    onProgress?.('正在启动 Anki，请稍候…');
    await utils.invoke<void>('launch_anki', { anki_executable_path: cfg.ankiExecutablePath || null });
    const deadline = Date.now() + ANKI_LAUNCH_WAIT_TIMEOUT_MS;
    while (true) {
        try {
            await probeAnkiConnect();
            return;
        } catch {
            // 本次探活失败，等待后重试
        }
        if (Date.now() >= deadline) {
            throw new Error(
                '等待 AnkiConnect 就绪超时。Anki 可能仍在启动中，' +
                '或未安装 AnkiConnect 插件（安装代码 2055492159），请检查后重试。'
            );
        }
        await sleep(ANKI_LAUNCH_POLL_INTERVAL_MS);
    }
}

/**
 * 确保 AnkiConnect 服务可用：探活失败且 Anki 未运行时，按配置自动拉起 Anki 并等待其就绪。
 *
 * 并发调用会共享同一次执行（后到的调用等待同一次结果，不会重复启动 Anki），
 * 此时只有第一次调用传入的 `onProgress` 会收到进度回调，启动行为也以第一次调用的参数为准。
 *
 * @param onProgress 进度提示回调（如“正在启动 Anki，请稍候……”）
 * @param options.forceLaunch 为 true 时无视“自动启动 Anki”设置强制拉起
 *   （仅用于用户显式发起的启动，如设置页的“启动 Anki”按钮）；缺省 false，隐式启动受该设置约束
 * @throws AnkiConnect 最终不可用时抛出 Error
 */
export async function ensureAnkiConnect(
    onProgress?: (message: string) => void,
    options?: { forceLaunch?: boolean }
): Promise<void> {
    if (ensureAnkiConnectInFlight == null) {
        ensureAnkiConnectInFlight = ensureAnkiConnectImpl(onProgress, options?.forceLaunch ?? false).finally(() => {
            ensureAnkiConnectInFlight = null;
        });
    }
    return ensureAnkiConnectInFlight;
}
// #endregion

// #region app version
export interface LatestAppInfo {
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

    if (debug.appUpdateScenario !== 'real') {
        // mock 分支：不读写持久化缓存，避免调试数据污染真实缓存
        latestAppInfo.value = await debug.mockLatestAppInfo();
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
    if (debug.templateVersionScenario === 'low-version') {
        // mock：模拟 Anki 中的笔记模板版本过低
        await debug.mockDelay();
        templateVersion.value = '0.0.0';
        return;
    }
    if (debug.templateVersionScenario === 'request-error') {
        // mock：模拟获取笔记模板版本失败
        await debug.mockDelay();
        templateVersion.value = new Error("mock request error (debug template version scenario: 'request-error')");
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

/** 是否已经初始化过 */
let initializedAtAppStart = false;

export async function initAtAppStart() {
    if (initializedAtAppStart) {
        return;
    }
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
    // 用配置文件中的主题模式校正 localStorage 缓存（首帧与 initTheme 用的是缓存值，可能与此处漂移）
    setThemeMode(config.theme);
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
    // dev 模式且真实场景（'real'）下禁用启动检查，避免频繁请求 GitHub API；
    // mock 场景不打真实 API，正常执行启动检查，以便调试红点等启动时 UI。
    // 距上次成功检查未超过 AUTO_CHECK_MIN_INTERVAL 时会直接使用持久化缓存，不发请求
    void (async () => {
        if (!debug.startupAppUpdateCheckEnabled()) {
            return;
        }
        try {
            await fetchAndSetLatestAppInfo();
        } catch (error) {
            console.error(error);
            // 应用更新检查失败时仅在控制台报错，不弹窗提示，也不阻止后续操作
        }
    })();
    // 配置开启时，在应用启动后自动启动 Anki（不等待结果，静默失败，不阻塞应用启动）
    if (config.launchAnkiOnAppStart) {
        void (async () => {
            try {
                await ensureAnkiConnect();
            } catch (error) {
                console.error(error);
                // 启动时自动拉起 Anki 失败仅在控制台报错，不弹窗提示，也不阻止后续操作
            }
        })();
    }
    // 初始化全局快捷键/辅助功能权限状态（挂常驻监听并主动查询一次，不阻塞启动）
    void initShortcutStatus();
    initializedAtAppStart = true;
}
