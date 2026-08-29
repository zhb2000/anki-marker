/**
 * 设置页的统一状态仓库：ConfigModel 的响应式镜像 + 自动保存。
 *
 * - 页面直接 v-model 读写 state，不再手写镜像 ref 与防抖保存逻辑
 * - state 与 Config 单例之间的同步：
 *   1. init / syncFromConfig：Config → state（全键拷贝）
 *   2. watch(state)：state → Config（遍历 CONFIG_KEYS 找 diff 逐键写入），并防抖 commit 落盘
 * - 与现状一致：不监听 'config-changed'，页面驻留期间不同步外部修改，进入设置页时 syncFromConfig
 */

import { reactive, ref, watch, type Ref } from 'vue';
import * as api from '../tauri-api';

import { CONFIG_DEFAULTS, CONFIG_KEYS, type Config, type ConfigModel } from './config';
import * as globals from './globals';
import { debounce } from './utils';

/** 自动保存的防抖间隔（毫秒）：停止修改一段时间后自动保存，缩小未保存修改的窗口期 */
const COMMIT_DEBOUNCE_MS = 500;

export interface SettingsStore {
    /** 配置项的响应式镜像，页面直接 v-model 读写 */
    readonly state: ConfigModel;
    /** init 是否已完成（页面在 ready 前不渲染，避免闪烁默认值） */
    readonly ready: Ref<boolean>;
    /** 幂等初始化：await globals.getConfig() 后把全部键拷贝进 state，并启动 watch */
    init(): Promise<void>;
    /** 从 config 全量回拷 state（进入设置页 onActivated 时调用，同步外部修改） */
    syncFromConfig(): void;
    /** 取消防抖并立即 commit（离开设置页/返回按钮时调用） */
    flush(): Promise<void>;
    /** 重置某键为默认值（写 state 即触发自动保存） */
    reset<K extends keyof ConfigModel>(key: K): void;
    /** 某设置项的当前值是否偏离默认值（决定单项重置按钮是否显示） */
    isModified<K extends keyof ConfigModel>(key: K): boolean;
    /** 将全部设置恢复为默认值（写 state 即触发自动保存） */
    resetAll(): void;
}

/**
 * 将 source 中指定键的值同步到 target。
 * 以泛型键参数而非联合类型索引赋值，避免 TypeScript 将联合键的写入类型收窄为 never
 * （同 config.ts 的 syncConfigKey）。
 */
function syncStoreKey<K extends keyof ConfigModel>(target: ConfigModel, source: ConfigModel, key: K): void {
    target[key] = source[key];
}

function createSettingsStore(): SettingsStore {
    /** 配置项的响应式镜像，初始为默认值，init 时从 config 拷贝真实值 */
    const state = reactive({ ...CONFIG_DEFAULTS }) as ConfigModel;
    const ready = ref(false);
    let config: Config | null = null;

    /** 提交 config 落盘，失败时弹窗报错（文案沿用原设置页） */
    async function commitConfig(): Promise<void> {
        if (config == null) {
            return;
        }
        try {
            await config.commit();
        } catch (error) {
            console.error(error);
            await api.dialog.message(String(error), { title: '配置文件保存失败', kind: 'error' });
        }
    }

    /** 防抖自动保存：连续修改期间持续触发会重置计时，停止一段时间后自动保存 */
    const debouncedCommitConfig = debounce(() => void commitConfig(), COMMIT_DEBOUNCE_MS);

    function syncFromConfig(): void {
        if (config == null) {
            return;
        }
        for (const key of CONFIG_KEYS) {
            syncStoreKey(state, config, key);
        }
    }

    /** 幂等初始化：并发/重复调用共享同一次执行 */
    let initPromise: Promise<void> | null = null;

    function init(): Promise<void> {
        if (initPromise == null) {
            initPromise = (async () => {
                config = await globals.getConfig();
                syncFromConfig();
                // state 的任何修改：找出与 config 不一致的键写入 config（Config 的 setter 自带脏标记，
                // 重复写同值无开销），然后防抖保存；键都是原始值，无需 deep watch
                watch(state, () => {
                    if (config == null) {
                        return;
                    }
                    let changed = false;
                    for (const key of CONFIG_KEYS) {
                        if (state[key] !== config[key]) {
                            syncStoreKey(config, state, key);
                            changed = true;
                        }
                    }
                    if (changed) {
                        debouncedCommitConfig();
                    }
                });
                ready.value = true;
            })();
        }
        return initPromise;
    }

    async function flush(): Promise<void> {
        debouncedCommitConfig.cancel(); // 取消待触发的防抖保存，立即保存以免离开设置页后才执行
        await commitConfig();
    }

    function reset<K extends keyof ConfigModel>(key: K): void {
        // 配置项的值类型为 string 或 boolean（见 CONFIG_DEFAULTS），断言到具体键的属性类型
        state[key] = CONFIG_DEFAULTS[key] as ConfigModel[K];
    }

    function isModified<K extends keyof ConfigModel>(key: K): boolean {
        return state[key] !== CONFIG_DEFAULTS[key];
    }

    function resetAll(): void {
        for (const key of CONFIG_KEYS) {
            reset(key);
        }
    }

    return { state, ready, init, syncFromConfig, flush, reset, isModified, resetAll };
}

/** 模块级单例 */
let store: SettingsStore | null = null;

/** 获取设置页状态仓库的模块级单例 */
export function useSettingsStore(): SettingsStore {
    if (store == null) {
        store = createSettingsStore();
    }
    return store;
}
