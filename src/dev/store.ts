/**
 * dev 设置的加载、持久化与对外接口。
 *
 * 取值优先级（高到低）：
 * 1. 会话覆盖：URL 参数 `?dev.<key>=<value>`、控制台 `window.__dev.set(key, value)`。
 *    只对当前页面有效、不写文件，供 AI/脚本临时切换而不碰设置文件；
 * 2. 文件：项目根目录的 dev-settings.local.json（gitignore），由设置页写入，
 *    经 Vite 插件的 /__dev/settings 读写，Tauri 窗口与浏览器模式共用同一份；
 * 3. 注册表默认值。
 *
 * 文件只保存偏离默认值的键，且在外部被编辑时（文件监听 → HMR 自定义事件）实时同步到页面。
 */
import { currentDevEnvironment } from './environment';
import {
    DEV_SETTING_DEFS, DEV_SETTING_KEYS, coerceDevSetting, isDevSettingKey,
    parseDevSettingText, type DevSettingKey, type DevSettings,
} from './schema';
import { devMockStatus, type DevMockStatus } from './status';
import { devOverridden, devSettings } from './state';

const ENDPOINT = '/__dev/settings';
/** 写请求必须带的自定义头：触发跨域预检，使其他网页无法跨站改写本机的 dev 设置 */
const WRITE_HEADER = { 'X-Dev-Settings': '1' };
/** 页面连续修改后延迟多久写文件（毫秒） */
const SAVE_DEBOUNCE_MS = 300;

/** 文件中的原始内容（未校验） */
let fileValues: Record<string, unknown> = {};
/** 会话覆盖（已校验） */
const overrides: Partial<DevSettings> = {};
/** 待写入文件的修改；值为 null 表示删除该键（恢复默认） */
let pendingWrite: Record<string, unknown> = {};
let saveTimer: ReturnType<typeof setTimeout> | undefined;

/** 按优先级重新计算全部键的生效值 */
function applyAll(): void {
    const target = devSettings as unknown as Record<string, unknown>;
    for (const key of DEV_SETTING_KEYS) {
        target[key] = overrides[key] ?? coerceDevSetting(key, fileValues[key]) ?? DEV_SETTING_DEFS[key].default;
        if (key in overrides) {
            devOverridden[key] = true;
        } else {
            delete devOverridden[key];
        }
    }
}

async function loadFile(): Promise<void> {
    try {
        const response = await fetch(ENDPOINT);
        if (response.ok) {
            const data: unknown = await response.json();
            fileValues = typeof data === 'object' && data !== null && !Array.isArray(data)
                ? data as Record<string, unknown>
                : {};
        }
    } catch (error) {
        console.warn('[dev] 读取 dev 设置文件失败，使用默认值', error);
    }
}

async function flushWrite(): Promise<void> {
    saveTimer = undefined;
    const body = pendingWrite;
    pendingWrite = {};
    if (Object.keys(body).length === 0) {
        return;
    }
    try {
        const response = await fetch(ENDPOINT, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json', ...WRITE_HEADER },
            body: JSON.stringify(body),
        });
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}`);
        }
    } catch (error) {
        console.error('[dev] 写入 dev 设置文件失败', error);
    }
}

/** 记录一次文件修改并延迟落盘；值等于默认值时从文件中删除该键 */
function queueWrite<K extends DevSettingKey>(key: K, value: DevSettings[K]): void {
    const isDefault = value === DEV_SETTING_DEFS[key].default;
    pendingWrite[key] = isDefault ? null : value;
    if (isDefault) {
        delete fileValues[key];
    } else {
        fileValues[key] = value;
    }
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => void flushWrite(), SAVE_DEBOUNCE_MS);
}

/** 设置页修改某项：写入文件并立即生效。会话覆盖了该键时，页面上的显式修改优先，覆盖随之解除 */
export function setDevSetting<K extends DevSettingKey>(key: K, value: DevSettings[K]): void {
    const valid = coerceDevSetting(key, value);
    if (valid === undefined) {
        throw new Error(`dev 设置 ${key} 的值不合法：${String(value)}`);
    }
    delete overrides[key];
    queueWrite(key, valid);
    applyAll();
}

/** 恢复某项默认值 */
export function resetDevSetting(key: DevSettingKey): void {
    delete overrides[key];
    queueWrite(key, DEV_SETTING_DEFS[key].default as never);
    applyAll();
}

/** 恢复全部 dev 设置的默认值（同时清除会话覆盖） */
export function resetAllDevSettings(): void {
    for (const key of DEV_SETTING_KEYS) {
        delete overrides[key];
        queueWrite(key, DEV_SETTING_DEFS[key].default as never);
    }
    applyAll();
}

/** 会话覆盖：只对当前页面有效，不写文件。返回的是校验后的值 */
function setOverride(key: string, value: unknown): unknown {
    if (!isDevSettingKey(key)) {
        throw new Error(`未知的 dev 设置：${key}。可用的键：${DEV_SETTING_KEYS.join(', ')}`);
    }
    const valid = typeof value === 'string' && typeof DEV_SETTING_DEFS[key].default !== 'string'
        ? parseDevSettingText(key, value)
        : coerceDevSetting(key, value);
    if (valid === undefined) {
        throw new Error(`dev 设置 ${key} 的值不合法：${String(value)}`);
    }
    (overrides as Record<string, unknown>)[key] = valid;
    applyAll();
    return valid;
}

function clearOverride(key?: string): void {
    if (key === undefined) {
        for (const k of DEV_SETTING_KEYS) {
            delete overrides[k];
        }
    } else if (isDevSettingKey(key)) {
        delete overrides[key];
    }
    applyAll();
}

/** 读取 URL 参数 `dev.<key>=<value>` 作为会话覆盖，非法的忽略并告警 */
function readUrlOverrides(): void {
    const params = new URLSearchParams(window.location.search);
    for (const [name, text] of params) {
        if (!name.startsWith('dev.')) {
            continue;
        }
        const key = name.slice('dev.'.length);
        const valid = isDevSettingKey(key) ? parseDevSettingText(key, text) : undefined;
        if (!isDevSettingKey(key) || valid === undefined) {
            console.warn(`[dev] 忽略无效的 URL 参数 ${name}=${text}`);
            continue;
        }
        (overrides as Record<string, unknown>)[key] = valid;
    }
}

/** 给 AI/脚本用的清单：每个 dev 设置的说明、可选值、默认值、当前值与来源 */
function describeAll() {
    return DEV_SETTING_KEYS.map(key => {
        const def = DEV_SETTING_DEFS[key];
        return {
            key,
            title: def.title,
            description: def.description,
            group: def.group,
            type: def.type,
            options: def.type === 'enum' ? def.options.map(option => option.value) : undefined,
            range: def.type === 'number' ? [def.min, def.max] : undefined,
            default: def.default,
            value: devSettings[key],
            source: key in overrides ? 'session' : key in fileValues ? 'file' : 'default',
        };
    });
}

declare global {
    interface Window {
        /** 开发者接口，仅在 dev 功能开启时存在，用法见 docs/dev-features.md */
        __dev?: {
            /** 列出全部 dev 设置（含可选值、当前值与来源） */
            list: typeof describeAll;
            /** 读取当前生效的全部 dev 设置 */
            get: () => DevSettings;
            /** 临时覆盖某项（只对当前页面有效、不写文件）；值也可以是 URL 参数那样的文本 */
            set: (key: string, value: unknown) => unknown;
            /** 清除会话覆盖；不传键则清除全部 */
            reset: (key?: string) => void;
            /** 清空 AI 模拟状态（结果缓存与参数兼容缓存），让下一次请求按当前场景重走 */
            resetMock?: () => void;
            /** 当前环境下所有实际生效的模拟、无效值警告与禁用项 */
            mockStatus: () => DevMockStatus;
            /** 是否运行在浏览器模式（没有 Tauri，由 src/dev/browser 提供夹具） */
            browserMode?: boolean;
            /** 浏览器模式：模拟后端向前端推送事件，如 `emit('sentence-captured', __dev.sampleCapture)` */
            emit?: (event: string, payload?: unknown) => Promise<void>;
            /** 浏览器模式：一份示例划词载荷 */
            sampleCapture?: unknown;
            /** 浏览器模式当前使用的后端：bridge = 真实 Rust（dev-bridge），fixtures = 夹具 */
            backend?: 'bridge' | 'fixtures';
            /** 返回当前游标；之后用 report(游标) 查看这期间发生了什么 */
            mark?: () => number;
            /** 原始观测条目 */
            logs?: (options?: { since?: number; level?: 'debug' | 'info' | 'warn' | 'error'; kind?: 'console' | 'error' | 'invoke'; }) => unknown[];
            /** 自 since 以来的错误、警告、失败的命令、调用轨迹、页面对话框与提示 */
            report?: (since?: number) => Promise<unknown>;
            /** Rust 日志文件尾部（仅桥接） */
            rustLogs?: (options?: { lines?: number; grep?: string; }) => Promise<string>;
            /** 用法说明 */
            help?: () => string;
            /** 按可见文字点击（也接受 CSS 选择器） */
            click?: (target: string, nth?: number) => string;
            /** 按 placeholder / 标签填写输入框 */
            fill?: (target: string, value: string, nth?: number) => string;
            /** 在目标（缺省为焦点元素）上按键 */
            press?: (key: string, target?: string) => string;
            /** 当前页面可见文字 */
            text?: (max?: number) => string;
            /** 等页面可见文字里出现某段文字（用于 idle 看不到的定时器式等待） */
            waitFor?: (pattern: string | RegExp, options?: { timeoutMs?: number; }) => Promise<{ found: boolean; waitedMs: number; }>;
            /** 等应用安静（无进行中的命令且 DOM 静止） */
            idle?: (options?: { quietMs?: number; timeoutMs?: number; }) => Promise<{ settled: boolean; waitedMs: number; }>;
        };
    }
}

/** 初始化：读文件 → 叠加 URL 覆盖 → 监听文件的外部修改 → 挂载 window.__dev */
export async function initDevStore(): Promise<void> {
    await loadFile();
    readUrlOverrides();
    applyAll();
    window.__dev = {
        list: describeAll,
        get: () => ({ ...devSettings }),
        set: setOverride,
        reset: clearOverride,
        mockStatus: () => devMockStatus(devSettings, currentDevEnvironment()),
    };
    // 文件被外部编辑（含 AI/脚本直接改文件）时同步；本页还有未写入的修改时先不覆盖
    import.meta.hot?.on('dev-settings-changed', () => {
        if (saveTimer !== undefined) {
            return;
        }
        void loadFile().then(applyAll);
    });
}