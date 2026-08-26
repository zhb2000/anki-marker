import * as api from '../tauri-api';

import { invoke } from './utils';

interface ConfigModel {
    ankiConnectURL: string;
    deckName: string;
    modelName: string;
    autoLaunchAnki: boolean;
    launchAnkiOnAppStart: boolean;
    ankiExecutablePath: string;
    globalShortcut: string;
    keepRunningOnClose: boolean;
    backgroundIcon: 'dock' | 'menu-bar' | 'none';
    llmEnabled: boolean;
    llmBaseUrl: string;
    llmApiKey: string;
    llmModel: string;
    llmReasoningEffort: string;
}

const CONFIG_KEYS = ['ankiConnectURL', 'deckName', 'modelName', 'autoLaunchAnki', 'launchAnkiOnAppStart', 'ankiExecutablePath', 'globalShortcut', 'keepRunningOnClose', 'backgroundIcon', 'llmEnabled', 'llmBaseUrl', 'llmApiKey', 'llmModel', 'llmReasoningEffort'] as const;

/** 配置项的默认值 */
export const CONFIG_DEFAULTS: Record<keyof ConfigModel, string | boolean> = {
    ankiConnectURL: 'http://localhost:8765',
    deckName: '划词助手默认牌组',
    modelName: '划词助手默认单词模板',
    autoLaunchAnki: true,
    launchAnkiOnAppStart: false,
    ankiExecutablePath: '',
    globalShortcut: '',
    keepRunningOnClose: true,
    backgroundIcon: 'dock',
    llmEnabled: false,
    llmBaseUrl: '',
    llmApiKey: '',
    llmModel: '',
    llmReasoningEffort: '',
};

/**
 * 将 source 中指定键的值同步到 target。
 * 以泛型键参数而非联合类型索引赋值，避免 TypeScript 将联合键的写入类型收窄为 never。
 */
function syncConfigKey<K extends keyof ConfigModel>(target: ConfigModel, source: ConfigModel, key: K): void {
    target[key] = source[key];
}

export class Config implements ConfigModel {
    /** The path of the configuration file. */
    public readonly path: string;
    /** Whether the app is in portable mode. */
    public readonly portable: boolean;
    /** Anki Connect 服务的 URL */
    public ankiConnectURL!: string;
    /** 将划词结果添加到的牌组名 */
    public deckName!: string;
    /** 划词结果使用的笔记模板名 */
    public modelName!: string;
    /** 添加笔记时若 Anki 未运行，是否自动启动 Anki */
    public autoLaunchAnki!: boolean;
    /** 应用启动时是否自动启动 Anki */
    public launchAnkiOnAppStart!: boolean;
    /** Anki 可执行文件的路径，留空表示自动检测 */
    public ankiExecutablePath!: string;
    /** 划词录入句子的全局快捷键，空字符串表示未设置 */
    public globalShortcut!: string;
    /** 关闭窗口时是否保持应用在后台运行（仅 macOS） */
    public keepRunningOnClose!: boolean;
    /** 后台运行期间应用图标的显示位置（仅 macOS）：dock=Dock 栏、menu-bar=菜单栏、none=都不显示 */
    public backgroundIcon!: 'dock' | 'menu-bar' | 'none';
    /** 是否启用 AI 优选释义（LLM） */
    public llmEnabled!: boolean;
    /** LLM 服务的 Base URL，留空表示未配置 */
    public llmBaseUrl!: string;
    /** LLM 服务的 API Key，留空表示未配置 */
    public llmApiKey!: string;
    /** LLM 使用的模型名，留空表示未配置 */
    public llmModel!: string;
    /** LLM 的推理强度（reasoning effort），留空表示使用模型默认值 */
    public llmReasoningEffort!: string;
    /** 存储配置项的对象 */
    private config: ConfigModel;
    /** 被修改过的配置项 */
    private modified: Partial<ConfigModel>;
    /** commit 的串行化队列（自身永不 reject），reload 读取文件前等待其清空 */
    private commitTail: Promise<void> = Promise.resolve();
    // Config 对象被设计为始终存活的全局单例，因此不需要取消事件监听
    /** 'config-changed' 事件对应的取消监听函数 */
    public __unlistenConfigChanged?: () => void;
    /** 'config-watcher-error' 事件对应的取消监听函数 */
    public __unlistenConfigWatcherError?: () => void;

    private constructor(config: ConfigModel, path: string, portable: boolean) {
        this.config = config;
        this.modified = {};
        this.path = path;
        this.portable = portable;
        for (const key of CONFIG_KEYS) {
            this.defineAccessor(key);
        }
    }

    private defineAccessor<K extends keyof ConfigModel>(propertyName: K): void {
        Object.defineProperty(this, propertyName, {
            get(this: Config): ConfigModel[K] {
                return this.config[propertyName];
            },
            set(this: Config, value: ConfigModel[K]) {
                if (value !== this.config[propertyName]) {
                    this.config[propertyName] = value;
                    this.modified[propertyName] = value;
                }
            },
            enumerable: true,
            configurable: true,
        });
    }

    public commit(): Promise<void> {
        // 串行化写入：并发的 commit（如防抖保存与 blur 保存撞车）排队依次执行，
        // 避免 Rust 端并发的读-改-写交错导致先完成的写入被后完成的覆盖
        const next = this.commitTail.then(() => this.commitImpl());
        this.commitTail = next.catch(() => { }); // 链条自身不因失败而中断，异常只传给调用方
        return next;
    }

    private async commitImpl(): Promise<void> {
        // 快照待写入的修改并立即清空脏标记：
        // 写入期间用户的新修改会记入新的 modified，不会随本次写入的完成而丢失
        const pending = this.modified;
        if (Object.keys(pending).length === 0) {
            return;
        }
        this.modified = {};
        try {
            await invoke('commit_config', { modified: pending, config_path: this.path });
        } catch (error) {
            // 写入失败：恢复未落盘的修改；写入期间用户又改过的键保留用户的最新值
            this.modified = { ...pending, ...this.modified };
            throw error;
        }
    }

    public async reload() {
        // 等待已排队的写入全部完成后再读取文件，避免读到写入前的旧内容
        // （链条不会 reject；等到写入失败的 commit 完成也无妨，读到的是文件的真实状态）
        await this.commitTail;
        const newConfig = await Config.load();
        // 键级合并（与 commit_config 只写入被修改键的文件写入策略对称）：
        // - 文件值与内存值一致：该键已落盘，清除其脏标记（典型场景：自己 commit 后 watcher 的回声）
        // - 文件值与内存值不一致、且该键不在 modified 中：外部修改，同步到内存
        // - 文件值与内存值不一致、且该键在 modified 中：用户尚未保存的修改，保留 UI 值，
        //   待 commit 时以用户输入为准——绝不回退正在编辑的内容
        for (const key of CONFIG_KEYS) {
            if (this.config[key] === newConfig.config[key]) {
                delete this.modified[key];
            } else if (!(key in this.modified)) {
                syncConfigKey(this.config, newConfig.config, key);
            }
        }
    }

    /**
     * 启动配置文件监视器。
     * Return true if the watcher is started successfully, false if it's already started.
     */
    public async startWatcher(): Promise<boolean> {
        return startConfigWatcher(this);
    }

    public static async load(): Promise<Config> {
        const [config_path, cfg, portable] = await Promise.all([
            invoke<string>('config_path'),
            invoke<ConfigModel>('read_config'),
            invoke<boolean>('is_portable')
        ]);
        return new Config(cfg, config_path, portable);
    }
}

export async function showInExplorer(path: string) {
    await invoke('show_in_explorer', { path });
}

export async function openFile(path: string) {
    await invoke('open_filepath', { path });
}

export async function openInBrowser(url: string | null | undefined) {
    if (url != null) {
        await invoke('open_in_browser', { url });
    }
}

/**
 * 启动配置文件监视器。
 * Return true if the watcher is started successfully, false if it's already started.
 */
export async function startConfigWatcher(config: Config): Promise<boolean> {
    const newWatcherStarted = await invoke<boolean>('start_config_watcher');
    // listeners set in the front-end will be removed after the page is reloaded
    if (config.__unlistenConfigChanged == null) {
        // 监听 'config-changed' 事件，以便在配置文件被修改时重新加载配置
        config.__unlistenConfigChanged = await api.event.listen('config-changed', () => {
            config.reload().catch(console.error);
        });
    }
    if (config.__unlistenConfigWatcherError == null) {
        // 监听 'config-watcher-error' 事件，以便在配置文件监视器出错时输出错误信息
        config.__unlistenConfigWatcherError = await api.event.listen('config-watcher-error', () => {
            console.error('Config watcher error');
        });
    }
    return newWatcherStarted;
}
