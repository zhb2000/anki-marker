/**
 * 开发者设置的“当前状态”推导：哪些项在当前运行环境下完全无效、哪些值在当前设置下无效、
 * 以及当前实际生效的模拟项清单。
 *
 * 纯函数，不依赖 Vue 或 window；设置页与 __dev.mockStatus() 共用同一份结果，
 * 保证人类看到的提示与 AI 拿到的清单不会各说各话。
 */
import { effectiveLlmScenario, type EffectiveLlmScenario } from './effective';
import {
    DEV_ENVIRONMENT_DESCRIPTIONS,
    DEV_ENVIRONMENT_LABELS,
    type DevEnvironment,
} from './environment';
import {
    DEV_SETTING_DEFS,
    DEV_SETTING_KEYS,
    type DevSettingKey,
    type DevSettings,
} from './schema';

/** 单个设置项在当前环境/其他设置下的可用性与提示 */
export interface DevSettingStatus {
    /** false 表示这一项在当前模式下完全无效，应禁用 */
    enabled: boolean;
    /** 禁用原因（enabled 为 false 时有值） */
    disabledReason?: string;
    /** 当前值在当前设置下无效或不推荐时的提示（不禁用） */
    warning?: string;
    /** 自动解析出来的实际生效值（如 LLM 场景的 auto） */
    effectiveValue?: string;
}

/** 一条当前实际生效的模拟 */
export interface DevMockEntry {
    key: DevSettingKey;
    title: string;
    value: string;
    detail: string;
}

export interface DevMockStatus {
    environment: DevEnvironment;
    environmentLabel: string;
    environmentDescription: string;
    /** 当前实际生效的模拟项（含环境级的夹具） */
    mocks: DevMockEntry[];
    /** 当前值无效/需要注意的设置项 */
    warnings: { key: DevSettingKey; message: string; }[];
    /** 在当前模式下完全无效、已禁用的设置项 */
    disabled: { key: DevSettingKey; reason: string; }[];
    /** 每个设置项的状态，供设置页逐卡渲染 */
    settings: Record<DevSettingKey, DevSettingStatus>;
    /** 自动解析后的实际值 */
    effective: { llmScenario: EffectiveLlmScenario; };
}

/** 枚举/布尔/数字的可读值 */
function labelOf(key: DevSettingKey, value: unknown): string {
    const def = DEV_SETTING_DEFS[key];
    if (def.type === 'enum') {
        return def.options.find(option => option.value === value)?.label ?? String(value);
    }
    if (def.type === 'boolean') {
        return value === true ? '开启' : '关闭';
    }
    return String(value);
}

/** 计算当前环境下的完整状态；settings 传 devSettings 或默认值即可 */
export function devMockStatus(settings: DevSettings, environment: DevEnvironment): DevMockStatus {
    const browser = environment !== 'tauri';
    const bridge = environment === 'browser-bridge';
    const fixtures = environment === 'browser-fixtures';
    const llmScenario = effectiveLlmScenario(settings, environment);
    const llmIsMock = llmScenario !== 'real';

    const perSetting = {} as Record<DevSettingKey, DevSettingStatus>;
    for (const key of DEV_SETTING_KEYS) {
        perSetting[key] = { enabled: true };
    }
    const disable = (key: DevSettingKey, disabledReason: string): void => {
        perSetting[key] = { ...perSetting[key], enabled: false, disabledReason };
    };
    const warn = (key: DevSettingKey, warning: string): void => {
        perSetting[key] = { ...perSetting[key], warning };
    };
    const effective = (key: DevSettingKey, effectiveValue: string): void => {
        perSetting[key] = { ...perSetting[key], effectiveValue };
    };

    // #region 环境限定项
    if (!browser) {
        disable('platform', 'Tauri 窗口使用真实操作系统；只有浏览器模式可以模拟平台');
        disable('browserBackend', 'Tauri 窗口没有浏览器后端；此设置只影响浏览器模式');
        disable('ankiState', 'Tauri 模式使用真实 Anki；此设置只影响浏览器模式');
    }
    if (!bridge) {
        disable('bridgeConfigWrites', fixtures
            ? '夹具模式没有桥接，配置只存在于浏览器会话'
            : 'Tauri 模式直接读写真实配置文件；此设置只影响浏览器桥接');
    }
    // #endregion

    // #region 浏览器与环境
    if (browser) {
        const actual = bridge ? '桥接' : '夹具';
        if (settings.browserBackend === 'fixtures' && bridge) {
            warn('browserBackend', '当前页面仍是桥接；把后端改为“仅夹具”后需要刷新页面');
        }
        effective('browserBackend', `实际：${actual}`);
        if (fixtures) {
            effective('bridgeConfigWrites', '仅浏览器会话');
        } else if (settings.bridgeConfigWrites === 'real') {
            warn('bridgeConfigWrites', '会写入真实配置文件（含 API Key）；调试时请确认这是你想要的');
        } else {
            effective('bridgeConfigWrites', '仅浏览器会话');
        }
        if (settings.platform !== 'auto') {
            effective('platform', `${labelOf('platform', settings.platform)}（模拟）`);
        }
    }
    // #endregion

    // #region Anki
    if (browser) {
        if (fixtures && settings.ankiState === 'real') {
            warn('ankiState', '夹具模式没有真实 Anki 连接；选择“真实 Anki”仍会使用夹具响应');
        }
        if (settings.ankiState !== 'real') {
            effective('ankiState', labelOf('ankiState', settings.ankiState));
        }
    }
    if (settings.templateVersionScenario === 'real') {
        if (fixtures) {
            warn('templateVersionScenario', '夹具模式的 AnkiConnect 也是夹具；不会读取真实 Anki 模板');
        } else if (browser && settings.ankiState !== 'real') {
            warn('templateVersionScenario', 'Anki 状态不是“真实 Anki”；模板版本仍会由夹具响应');
        }
    }
    if (settings.templateVersionScenario !== 'real') {
        effective('templateVersionScenario', `${labelOf('templateVersionScenario', settings.templateVersionScenario)}（模拟）`);
    }
    // #endregion

    // #region 更新
    if (fixtures && settings.appUpdateScenario === 'real') {
        warn('appUpdateScenario', '夹具模式没有真实网络；仍会返回夹具的 GitHub 响应');
    }
    if (settings.appUpdateScenario !== 'real') {
        effective('appUpdateScenario', `${labelOf('appUpdateScenario', settings.appUpdateScenario)}（模拟）`);
    }
    if (settings.appUpdateScenario !== 'real') {
        disable('realUpdateCheckAtStartup', '应用更新场景本身是模拟；启动检查不会因为开启此项而联网');
    }
    if (settings.appUpdateScenario === 'real' && settings.templateVersionScenario === 'real') {
        disable('mockDelayMs', '当前应用更新与模板版本都选择真实请求；模拟延迟只作用于模拟场景');
    }
    // #endregion

    // #region AI
    const llmReason = settings.llmScenario !== 'auto'
        ? '显式选择'
        : settings.mockLlmConfig
            ? '免 Key 预览 AI 强制模拟'
            : environment === 'tauri'
                ? 'Tauri 默认真实请求'
                : '浏览器默认模拟';
    effective('llmScenario', `${labelOf('llmScenario', llmScenario)} · ${llmReason}`);
    if (fixtures && llmScenario === 'real') {
        warn('llmScenario', '夹具模式没有真实网络；真实 LLM 请求会失败，请改用桥接/Tauri 或选择模拟场景');
    }
    if (settings.llmScenario === 'real') {
        disable('mockLlmConfig', 'LLM 场景已明确选择真实请求；免 Key 预览只对模拟场景生效');
    }
    if (!llmIsMock) {
        disable('llmStreamDelayMs', '当前 LLM 实际走真实请求；流式夹具不参与');
    }
    // #endregion

    // #region 当前生效的模拟
    const mocks: DevMockEntry[] = [];
    const addMock = (key: DevSettingKey, title: string, value: string, detail: string): void => {
        mocks.push({ key, title, value, detail });
    };

    if (fixtures) {
        addMock('browserBackend', '浏览器夹具', '全部命令与网络请求',
            '词典、AnkiConnect、配置读写、AI 与模型列表都由夹具响应');
    } else if (bridge) {
        addMock('browserBackend', '浏览器桥接', '桥接 + 局部夹具',
            '剪贴板、自启动、打开程序/目录、窗口与快捷键挂起恢复由夹具处理');
    }
    if (bridge && settings.bridgeConfigWrites === 'session') {
        addMock('bridgeConfigWrites', '配置写入', '仅浏览器会话',
            '读取真实配置；写入不落真实 config.toml');
    }
    if (llmIsMock) {
        addMock('llmScenario', 'AI / LLM', labelOf('llmScenario', llmScenario),
            'AI 优选、测试连接、模型列表由夹具响应，不访问真实 API');
    }
    if (settings.mockLlmConfig && llmIsMock) {
        addMock('mockLlmConfig', 'AI 连接配置', '模拟（免 Key）',
            '读取配置时注入假 LLM 地址、Key 与模型并启用 AI；不修改真实配置文件');
    }
    if (browser && settings.ankiState !== 'real') {
        addMock('ankiState', 'Anki', labelOf('ankiState', settings.ankiState),
            'AnkiConnect 请求与 Anki 启动探测由夹具响应，不会写入真实牌组');
    }
    if (settings.appUpdateScenario !== 'real') {
        addMock('appUpdateScenario', '应用更新', labelOf('appUpdateScenario', settings.appUpdateScenario),
            '更新检查返回固定结果，不访问 GitHub');
    }
    if (settings.templateVersionScenario !== 'real') {
        addMock('templateVersionScenario', '笔记模板版本', labelOf('templateVersionScenario', settings.templateVersionScenario),
            '模板版本读取由夹具响应，不连接真实 Anki');
    }
    if (browser && settings.platform !== 'auto') {
        addMock('platform', '操作系统', `${labelOf('platform', settings.platform)}（模拟）`,
            '平台专属设置、标题栏与快捷键显示按模拟平台渲染');
    }
    // #endregion

    const warnings = DEV_SETTING_KEYS
        .filter(key => perSetting[key].warning != null)
        .map(key => ({ key, message: perSetting[key].warning! }));
    const disabled = DEV_SETTING_KEYS
        .filter(key => !perSetting[key].enabled)
        .map(key => ({ key, reason: perSetting[key].disabledReason! }));

    return {
        environment,
        environmentLabel: DEV_ENVIRONMENT_LABELS[environment],
        environmentDescription: DEV_ENVIRONMENT_DESCRIPTIONS[environment],
        mocks,
        warnings,
        disabled,
        settings: perSetting,
        effective: { llmScenario },
    };
}
