import { open as shellOpen } from '@tauri-apps/api/shell';
import { listen } from '@tauri-apps/api/event';

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
    public ankiConnectURL!: string;
    public deckName!: string;
    public modelName!: string;
    private config: ConfigModel;
    private modified: Partial<ConfigModel>;
    public __unlistenChanged?: () => void;
    public __unlistenWatcherError?: () => void;

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
            get(this: Config) {
                return this.config[propertyName];
            },
            set(this: Config, value) {
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
    await shellOpen(path);
}

export async function startConfigWatcher(config: Config) {
    await invoke<boolean>('start_config_watcher');
    // listeners set in the front-end will be removed after the page is reloaded
    if (config.__unlistenChanged == null) {
        config.__unlistenChanged = await listen('config-changed', async () => {
            await config.reload();
        });
    }
    if (config.__unlistenWatcherError == null) {
        config.__unlistenWatcherError = await listen('config-watcher-error', () => {
            console.error('Config watcher error');
        });
    }
}
