import * as api from '../tauri-api';

import { invoke } from './utils';

interface ConfigModel {
    ankiConnectURL: string;
    deckName: string;
    modelName: string;
}

const CONFIG_KEYS = ['ankiConnectURL', 'deckName', 'modelName'] as const;

/** 配置项的默认值 */
export const CONFIG_DEFAULTS: Record<keyof ConfigModel, string> = {
    ankiConnectURL: 'http://localhost:8765',
    deckName: '划词助手默认牌组',
    modelName: '划词助手默认单词模板',
};

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
    /** 存储配置项的对象 */
    private config: ConfigModel;
    /** 被修改过的配置项 */
    private modified: Partial<ConfigModel>;
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

    private defineAccessor(propertyName: keyof ConfigModel): void {
        Object.defineProperty(this, propertyName, {
            get(this: Config): ConfigModel[typeof propertyName] {
                return this.config[propertyName];
            },
            set(this: Config, value: ConfigModel[typeof propertyName]) {
                if (value !== this.config[propertyName]) {
                    this.config[propertyName] = value;
                    this.modified[propertyName] = value;
                }
            },
            enumerable: true,
            configurable: true,
        });
    }

    public async commit() {
        if (Object.keys(this.modified).length === 0) {
            return;
        }
        await invoke('commit_config', { modified: this.modified, config_path: this.path });
        this.modified = {};
    }

    public async reload() {
        const newConfig = await Config.load();
        if (CONFIG_KEYS.some(key => this.config[key] !== newConfig.config[key])) {
            this.config = newConfig.config;
            this.modified = {};
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
