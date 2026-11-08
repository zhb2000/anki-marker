/**
 * 开发者设置（dev 设置）的注册表：全部 dev 功能的唯一定义处。
 *
 * 设置页、URL 参数、`window.__dev`、文件校验都由这张表驱动，新增一个 dev 设置只需要：
 * 1. 在 DevSettings 加一个键；
 * 2. 在 DEV_SETTING_DEFS 加对应的定义（标题、说明、默认值、取值范围）；
 * 3. 在业务代码里经 logics/debug.ts 读取。
 *
 * dev 设置只在 dev 构建且总开关 VITE_DEV_FEATURES 打开时存在，见 docs/dev-features.md。
 * 默认值原则：不产生外部副作用的读取/展示尽量与真实行为一致；会产生副作用的外部调用
 * （Anki 写入、付费 LLM 请求）默认由夹具模拟，只有显式选择“真实”才联网。
 */

/** 应用更新检查的场景 */
export type AppUpdateScenario = 'real' | 'new-version' | 'no-update' | 'request-error';
/** 笔记模板版本获取的场景 */
export type TemplateVersionScenario = 'real' | 'low-version' | 'request-error';
/**
 * AI 优选 / LLM 请求的场景。
 *
 * 'auto' 按当前运行环境取默认值：Tauri 应用默认真实请求，浏览器（桥接/夹具）默认模拟。
 * 显式选 'real' 才会在所有环境下都使用配置中的真实地址与 API Key 发起付费请求。
 */
export type LlmScenario =
    | 'auto'
    | 'real'
    | 'pick'
    | 'thinking'
    | 'fallback'
    | 'sentence'
    | 'misspelling'
    | 'gibberish'
    | 'bad-json'
    | 'http-error'
    | 'empty'
    | 'compat-retry'
    | 'bad-pick-retry';
/** 浏览器模式下的操作系统（auto 为跟随浏览器所在系统） */
export type MockPlatform = 'auto' | 'macos' | 'windows' | 'linux';
/** 浏览器模式的后端：auto = 有桥接就连真实 Rust，否则用夹具；fixtures = 始终用夹具 */
export type BrowserBackend = 'auto' | 'fixtures';
/** Anki 与 AnkiConnect 的状态（real 仅在桥接下有意义：连接真实的 Anki，会写入真实的牌组） */
export type MockAnkiState = 'ok' | 'anki-not-running' | 'plugin-missing' | 'real';
/** 桥接模式下配置的写入位置：session = 只写浏览器会话（不改本机配置文件）；real = 写真实配置文件 */
export type BridgeConfigWrites = 'session' | 'real';

export interface DevSettings {
    appUpdateScenario: AppUpdateScenario;
    templateVersionScenario: TemplateVersionScenario;
    llmScenario: LlmScenario;
    llmStreamDelayMs: number;
    mockLlmConfig: boolean;
    launchAtLoginEditable: boolean;
    realUpdateCheckAtStartup: boolean;
    mockDelayMs: number;
    platform: MockPlatform;
    ankiState: MockAnkiState;
    browserBackend: BrowserBackend;
    bridgeConfigWrites: BridgeConfigWrites;
}

export type DevSettingKey = keyof DevSettings;

/** 设置页中的分组 */
export type DevSettingGroup = 'ai' | 'anki' | 'updates' | 'browser' | 'behavior';

export const DEV_GROUP_TITLES: Record<DevSettingGroup, string> = {
    ai: 'AI 与 LLM',
    anki: 'Anki 与模板',
    updates: '更新与版本',
    browser: '浏览器模式',
    behavior: '其他行为',
};

interface DefBase {
    title: string;
    description: string;
    group: DevSettingGroup;
}

export interface EnumDef<T extends string> extends DefBase {
    type: 'enum';
    default: T;
    options: readonly { value: T; label: string; }[];
}

export interface BoolDef extends DefBase {
    type: 'boolean';
    default: boolean;
}

export interface NumberDef extends DefBase {
    type: 'number';
    default: number;
    min: number;
    max: number;
    unit?: string;
}

type DefOf<V> = [V] extends [boolean] ? BoolDef
    : [V] extends [number] ? NumberDef
    : EnumDef<Extract<V, string>>;

export type DevSettingDef = BoolDef | NumberDef | EnumDef<string>;

export const DEV_SETTING_DEFS: { [K in DevSettingKey]: DefOf<DevSettings[K]> } = {
    appUpdateScenario: {
        type: 'enum',
        group: 'updates',
        title: '应用更新检查',
        description: '模拟 GitHub Release 检查的结果；非“真实请求”时不联网，切换后立即重新检查',
        default: 'real',
        options: [
            { value: 'real', label: '真实请求' },
            { value: 'new-version', label: '有新版本' },
            { value: 'no-update', label: '已是最新' },
            { value: 'request-error', label: '请求失败' },
        ],
    },
    templateVersionScenario: {
        type: 'enum',
        group: 'anki',
        title: 'Anki 笔记模板版本',
        description: '模拟从 Anki 获取到的笔记模板版本，切换后立即重新获取',
        default: 'real',
        options: [
            { value: 'real', label: '真实获取' },
            { value: 'low-version', label: '版本过低' },
            { value: 'request-error', label: '获取失败' },
        ],
    },
    llmScenario: {
        type: 'enum',
        group: 'ai',
        title: 'LLM / AI 优选',
        description: '跟随环境：Tauri 应用默认真实请求，浏览器桥接/夹具默认模拟；显式选择可覆盖。真实请求会使用配置中的真实 API Key 并产生费用',
        default: 'auto',
        options: [
            { value: 'auto', label: '跟随环境（默认）' },
            { value: 'real', label: '真实请求' },
            { value: 'pick', label: '命中候选' },
            { value: 'thinking', label: '思考模型（先输出思维链）' },
            { value: 'fallback', label: '未命中候选（AI 释义）' },
            { value: 'sentence', label: '整句翻译' },
            { value: 'misspelling', label: '拼写错误' },
            { value: 'gibberish', label: '无法识别的乱码' },
            { value: 'bad-json', label: '返回非法 JSON' },
            { value: 'http-error', label: 'HTTP 错误' },
            { value: 'empty', label: '空响应' },
            { value: 'compat-retry', label: '参数降级后成功' },
            { value: 'bad-pick-retry', label: '编号错误后重试' },
        ],
    },
    mockLlmConfig: {
        type: 'boolean',
        group: 'ai',
        title: '免 Key 预览 AI',
        description: '读取配置时填入可用的假 LLM 地址、Key 与模型并启用 AI，不修改真实配置文件。跟随环境下开启它会强制使用模拟场景；LLM 场景显式选“真实请求”时此开关无效',
        default: false,
    },
    launchAtLoginEditable: {
        type: 'boolean',
        group: 'behavior',
        title: '开发构建下允许修改“登录时自动启动”',
        description: 'dev 与正式安装共用系统自启动注册，开启后会把 dev 二进制注册为自启动，仅在需要查看开启状态的界面时临时打开；浏览器模式下自启动由夹具模拟，开启只影响界面状态',
        default: false,
    },
    realUpdateCheckAtStartup: {
        type: 'boolean',
        group: 'updates',
        title: '启动时真实检查更新',
        description: 'GitHub Release API 有每小时配额，dev 默认不在启动时请求；与上方场景无关，场景为模拟时本就不联网',
        default: false,
    },
    mockDelayMs: {
        type: 'number',
        group: 'behavior',
        title: '模拟请求延迟',
        description: '模拟场景下假请求的耗时，用于观察加载中状态',
        default: 1000,
        min: 0,
        max: 10000,
        unit: '毫秒',
    },
    llmStreamDelayMs: {
        type: 'number',
        group: 'ai',
        title: 'AI 流式输出间隔',
        description: '模拟流式响应每块之间的延迟，用于观察思考、流式与停止；0 表示不延迟',
        default: 120,
        min: 0,
        max: 2000,
        unit: '毫秒',
    },
    platform: {
        type: 'enum',
        group: 'browser',
        title: '模拟操作系统',
        description: '决定平台专属的设置项与标题栏样式；页面加载时读取，修改后需刷新页面',
        default: 'auto',
        options: [
            { value: 'auto', label: '跟随浏览器' },
            { value: 'macos', label: 'macOS' },
            { value: 'windows', label: 'Windows' },
            { value: 'linux', label: 'Linux' },
        ],
    },
    ankiState: {
        type: 'enum',
        group: 'anki',
        title: 'Anki 状态',
        description: '模拟 Anki 与 AnkiConnect 的可用性，影响添加笔记、模板版本等需要连接 Anki 的流程。“真实 Anki”仅桥接下有效，会真的写入 Anki',
        default: 'ok',
        options: [
            { value: 'ok', label: '正常（模拟）' },
            { value: 'anki-not-running', label: 'Anki 未运行（模拟）' },
            { value: 'plugin-missing', label: 'Anki 运行中但 AnkiConnect 不可用（模拟）' },
            { value: 'real', label: '真实 Anki（仅桥接）' },
        ],
    },
    browserBackend: {
        type: 'enum',
        group: 'browser',
        title: '浏览器模式的后端',
        description: '自动：dev-bridge 在运行就转发到真实 Rust 命令（词典、配置读取、事件等），否则用夹具；也可强制只用夹具。页面加载时读取，修改后需刷新',
        default: 'auto',
        options: [
            { value: 'auto', label: '自动（有桥接用桥接）' },
            { value: 'fixtures', label: '仅夹具' },
        ],
    },
    bridgeConfigWrites: {
        type: 'enum',
        group: 'browser',
        title: '桥接下配置的写入位置',
        description: '默认只写浏览器会话，读取仍来自真实配置文件，避免调试时误改本机的开发配置（含 API Key）',
        default: 'session',
        options: [
            { value: 'session', label: '仅浏览器会话' },
            { value: 'real', label: '写入真实配置文件' },
        ],
    },
};

export const DEV_SETTING_KEYS = Object.keys(DEV_SETTING_DEFS) as DevSettingKey[];

/** 全部 dev 设置的默认值 */
export function defaultDevSettings(): DevSettings {
    const result: Record<string, unknown> = {};
    for (const key of DEV_SETTING_KEYS) {
        result[key] = DEV_SETTING_DEFS[key].default;
    }
    return result as unknown as DevSettings;
}

/** 校验一个值是否是该设置项的合法取值，合法则原样返回，否则返回 undefined */
export function coerceDevSetting<K extends DevSettingKey>(key: K, raw: unknown): DevSettings[K] | undefined {
    const def = DEV_SETTING_DEFS[key] as DevSettingDef;
    switch (def.type) {
        case 'boolean':
            return typeof raw === 'boolean' ? raw as DevSettings[K] : undefined;
        case 'number':
            return typeof raw === 'number' && Number.isFinite(raw) && raw >= def.min && raw <= def.max
                ? raw as DevSettings[K]
                : undefined;
        case 'enum':
            return typeof raw === 'string' && def.options.some(option => option.value === raw)
                ? raw as DevSettings[K]
                : undefined;
    }
}

/** 把 URL 参数里的文本解析为该设置项的值，不合法返回 undefined */
export function parseDevSettingText<K extends DevSettingKey>(key: K, text: string): DevSettings[K] | undefined {
    const def = DEV_SETTING_DEFS[key] as DevSettingDef;
    switch (def.type) {
        case 'boolean':
            return coerceDevSetting(key, text === 'true' ? true : text === 'false' ? false : undefined);
        case 'number':
            return coerceDevSetting(key, text.trim() === '' ? undefined : Number(text));
        case 'enum':
            return coerceDevSetting(key, text);
    }
}

/** 类型守卫：字符串是否是已注册的 dev 设置键 */
export function isDevSettingKey(key: string): key is DevSettingKey {
    return (DEV_SETTING_KEYS as string[]).includes(key);
}
