import { reactive, ref, watch, computed } from 'vue';
import * as api from '../tauri-api';
import { fetch } from '@tauri-apps/plugin-http';
import * as semver from 'semver';
import { dialog, setDialogDepthListener } from '../fluent-controls';

import { Config, effectiveTextSetting } from './config';
import * as anki from './anki';
import { AnkiService } from './anki';
import * as utils from './utils';
import { typeAssertion } from './typing';
import * as preference from './preference';
import * as debug from './debug';
import { initShortcutStatus } from './shortcut-status';
import { setThemeMode, isDark } from './theme';

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
    ankiService = new AnkiService(effectiveTextSetting('ankiConnectURL', cfg.ankiConnectURL));
    /** 监听 config 的 anki-connect-url 更新，并同步到 ankiService（留空视为使用内置默认） */
    watch(
        () => effectiveTextSetting('ankiConnectURL', config!.ankiConnectURL),
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
/** 启动后每隔多久确认一次进程还在（用户可能在启动过程中把 Anki 关掉） */
const ANKI_LAUNCH_RECHECK_INTERVAL_MS = 3000;
/** 一次等待里最多拉起几次 Anki（避免用户关掉后无限重开窗口） */
const ANKI_MAX_LAUNCH_ATTEMPTS = 3;
/** Anki 已在运行但连不上时，先给多久的宽限期（可能只是还没启动完，插件尚未就绪） */
const ANKI_RUNNING_GRACE_MS = 8000;
/**
 * 拉起 Anki 后，进程已经在、AnkiConnect 却迟迟不监听时，最多再等多久就提示检查插件。
 * 不必等到 60 秒总超时——插件没启用时等下去也不会变好。
 */
const ANKI_CONNECT_GRACE_MS = 20_000;
/** Anki 在运行却连不上时的统一提示（安装/启用插件的指引） */
const ANKI_CONNECT_PLUGIN_HINT =
    'Anki 正在运行，但无法连接 AnkiConnect。' +
    '请确认已安装并启用 AnkiConnect 插件（安装代码 2055492159），并检查 AnkiConnect 服务地址设置。';
/** ensureAnkiConnect 正在执行中的 Promise，用于防止重入（并发调用共享同一次执行） */
let ensureAnkiConnectInFlight: { promise: Promise<void>; controller: AbortController; } | null = null;
/**
 * 正在等待这次共享执行的调用方。
 * 最后一个等待者取消（例如用户点了卡片的「停止」）时中断共享执行：
 * 否则它会在后台继续轮询、甚至在用户关掉 Anki 后把它重新拉起来。
 */
const ankiConnectWaiters = new Set<object>();
/**
 * 共享执行期间登记的所有进度回调。
 * 并发添加多个词条时会共享同一次启动 Anki 的执行，此时每个等待中的卡片都应看到
 * 同一条进度（否则会出现“两张卡片都在处理中，只有一张显示进度条”的不一致）。
 */
const ankiConnectProgressListeners = new Set<(message: string) => void>();
/** 最近一次进度文案，用于给中途加入的调用补发，避免它静默等待 */
let lastAnkiConnectProgress: string | null = null;

/** 已取消等待时立刻退出（避免继续轮询/重新拉起 Anki） */
function throwIfAborted(signal: AbortSignal): void {
    if (signal.aborted) {
        throw new DOMException('已停止等待 Anki 启动', 'AbortError');
    }
}

/** 向所有等待中的调用广播进度 */
function emitAnkiConnectProgress(message: string): void {
    lastAnkiConnectProgress = message;
    for (const listener of ankiConnectProgressListeners) {
        listener(message);
    }
}

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

async function ensureAnkiConnectImpl(
    onProgress: ((message: string) => void) | undefined,
    forceLaunch: boolean,
    signal: AbortSignal,
): Promise<void> {
    // 先探活，AnkiConnect 可用时直接返回
    try {
        await probeAnkiConnect();
        return;
    } catch {
        // AnkiConnect 不可用，继续后续处理
    }
    throwIfAborted(signal);
    // Anki 在运行但连不上：可能是刚启动、AnkiConnect 还没就绪，先给一小段宽限期
    if (await utils.invoke<boolean>('is_anki_running')) {
        const graceDeadline = Date.now() + ANKI_RUNNING_GRACE_MS;
        while (Date.now() < graceDeadline) {
            await sleep(ANKI_LAUNCH_POLL_INTERVAL_MS);
            throwIfAborted(signal);
            try {
                await probeAnkiConnect();
                return;
            } catch {
                // 宽限期内继续探活
            }
        }
        throw new Error(ANKI_CONNECT_PLUGIN_HINT);
    }
    const cfg = await getConfig();
    // “自动启动 Anki”设置只约束隐式启动（如添加笔记时顺带拉起）；
    // 用户显式发起的启动（forceLaunch，如设置页的“启动 Anki”按钮）不受该设置约束
    if (!cfg.autoLaunchAnki && !forceLaunch) {
        throw new Error('无法连接 AnkiConnect，且未启用自动启动 Anki。请手动启动 Anki，或在设置中开启“自动启动 Anki”。');
    }
    // 拉起 Anki，然后串行轮询等待 AnkiConnect 就绪（上一次探活结束后再 sleep，避免请求堆积）
    // 注意：该命令标注了 rename_all = "snake_case"（项目惯例），参数 key 须用 snake_case
    const launchAnki = async () => {
        await utils.invoke<void>('launch_anki', { anki_executable_path: cfg.ankiExecutablePath || null });
    };

    throwIfAborted(signal);
    onProgress?.('正在启动 Anki，请稍候…');
    await launchAnki();
    let launchCount = 1;
    const deadline = Date.now() + ANKI_LAUNCH_WAIT_TIMEOUT_MS;
    // 刚拉起时 Anki 可能还没出现在进程表里，先等一小段再开始确认进程是否还在
    let nextRunningCheck = Date.now() + ANKI_LAUNCH_RECHECK_INTERVAL_MS;
    // 进程已经起来、只是 AnkiConnect 还没监听：文案切换成“连接中”，否则会一直停在“正在启动”
    let announcedConnecting = false;
    // 进入“连接中”后最多等这么久，超时直接提示检查插件，而不是耗到 60 秒总超时
    let connectingDeadline: number | null = null;
    while (true) {
        throwIfAborted(signal);
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
        // Anki 进程已经在、但一直等不到 AnkiConnect：多半是插件没装/没启用，提前给出指引
        if (connectingDeadline != null && Date.now() >= connectingDeadline) {
            throw new Error(ANKI_CONNECT_PLUGIN_HINT);
        }
        // 拉起后用户又把 Anki 关掉（或启动失败）：重新拉起，而不是一直等到超时
        if (Date.now() >= nextRunningCheck) {
            nextRunningCheck = Date.now() + ANKI_LAUNCH_RECHECK_INTERVAL_MS;
            const running = await utils.invoke<boolean>('is_anki_running');
            if (!running) {
                if (launchCount >= ANKI_MAX_LAUNCH_ATTEMPTS) {
                    throw new Error(
                        'Anki 启动后又退出了。请手动启动 Anki（并确认已启用 AnkiConnect 插件）后重试。'
                    );
                }
                launchCount += 1;
                throwIfAborted(signal);
                announcedConnecting = false;
                connectingDeadline = null;
                onProgress?.('正在重新启动 Anki，请稍候…');
                await launchAnki();
                nextRunningCheck = Date.now() + ANKI_LAUNCH_RECHECK_INTERVAL_MS;
            } else if (!announcedConnecting) {
                // Anki 进程已经在了，剩下的是等 AnkiConnect 开始监听端口
                announcedConnecting = true;
                connectingDeadline = Date.now() + ANKI_CONNECT_GRACE_MS;
                onProgress?.('正在连接 Anki，请稍候…');
            }
        }
        await sleep(ANKI_LAUNCH_POLL_INTERVAL_MS);
    }
}

/**
 * 确保 AnkiConnect 服务可用：探活失败且 Anki 未运行时，按配置自动拉起 Anki 并等待其就绪。
 *
 * 并发调用会共享同一次执行（后到的调用等待同一次结果，不会重复启动 Anki），
 * 进度会广播给所有等待中的调用，中途加入的调用会立刻收到当前进度，不会静默等待；
 * 启动行为（forceLaunch）以第一次调用为准。
 *
 * 取消语义：调用方传入 signal 后，中断 signal 只解除它自己的等待；当所有等待者都取消时，
 * 共享执行本身也会被中断——否则它会在后台继续轮询，甚至在用户关掉 Anki 后又把它拉起来。
 *
 * @param onProgress 进度提示回调（如“正在启动 Anki，请稍候……”）
 * @param options.forceLaunch 为 true 时无视“自动启动 Anki”设置强制拉起
 *   （仅用于用户显式发起的启动，如设置页的“启动 Anki”按钮）；缺省 false，隐式启动受该设置约束
 * @param options.signal 取消信号（见上）
 * @throws AnkiConnect 最终不可用时抛出 Error；被取消时抛出 AbortError
 */
export function ensureAnkiConnect(
    onProgress?: (message: string) => void,
    options?: { forceLaunch?: boolean; signal?: AbortSignal; }
): Promise<void> {
    const signal = options?.signal;
    const waiter = {};
    ankiConnectWaiters.add(waiter);

    if (onProgress != null) {
        ankiConnectProgressListeners.add(onProgress);
        // 已有执行且已经发出过进度（Anki 正在启动）：补发一次，让后到的卡片也显示进度
        if (ensureAnkiConnectInFlight != null && lastAnkiConnectProgress != null) {
            onProgress(lastAnkiConnectProgress);
        }
    }

    if (ensureAnkiConnectInFlight == null) {
        lastAnkiConnectProgress = null;
        const controller = new AbortController();
        const promise = ensureAnkiConnectImpl(
            emitAnkiConnectProgress,
            options?.forceLaunch ?? false,
            controller.signal,
        ).finally(() => {
            if (ensureAnkiConnectInFlight?.controller === controller) {
                ensureAnkiConnectInFlight = null;
            }
            lastAnkiConnectProgress = null;
            ankiConnectProgressListeners.clear();
            ankiConnectWaiters.clear();
        });
        ensureAnkiConnectInFlight = { promise, controller };
    }

    const current = ensureAnkiConnectInFlight;
    // 调用方取消：退出等待；最后一个等待者取消时中断共享执行，避免它在后台继续重试拉起 Anki
    signal?.addEventListener('abort', () => {
        ankiConnectWaiters.delete(waiter);
        if (onProgress != null) {
            ankiConnectProgressListeners.delete(onProgress);
        }
        if (ankiConnectWaiters.size === 0 && ensureAnkiConnectInFlight === current) {
            current.controller.abort();
        }
    }, { once: true });

    return current.promise;
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
    // 弹窗层数变化时同步 macOS 标题栏遮罩（见 window.rs set_dialog_mask；
    // 非 macOS / 非 Tauri 环境命令为 no-op 或调用失败，静默忽略）。
    // 传层数而不是布尔：页面上的每层弹窗各叠一层 smoke 遮罩，标题栏要按同样层数加深
    setDialogDepthListener(depth => {
        void utils.invoke('set_dialog_mask', { depth, dark: isDark.value }).catch(() => { /* 忽略 */ });
    });
    // 获取应用版本
    await initAppVersion();
    // 加载配置文件
    try {
        await initConfig();
    } catch (error) {
        console.error(error);
        await dialog.message(String(error), { title: '配置文件读取失败', kind: 'error' });
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
        await dialog.message(String(error), { title: '配置文件监听失败', kind: 'error' });
        // 配置文件监听失败时仅弹窗报错，不阻止后续操作
    }
    // 获取笔记模板版本，不等待结果。
    // 当模板名称或 AnkiConnect URL 改变时，重新获取模板版本（两者留空时取内置默认值）
    watch(
        // Use a getter function as the watch source to watch properties of a reactive object.
        // See https://vuejs.org/guide/essentials/watchers.html#watch-source-types
        () => [
            effectiveTextSetting('modelName', config.modelName),
            effectiveTextSetting('ankiConnectURL', config.ankiConnectURL),
        ],
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
    // 初始化全局快捷键/辅助功能权限状态（挂常驻监听并主动查询一次，不阻塞启动）
    void initShortcutStatus();
    initializedAtAppStart = true;
}
