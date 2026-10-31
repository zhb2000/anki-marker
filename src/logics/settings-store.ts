/**
 * 设置页的统一状态仓库：ConfigModel 的响应式镜像 + 自动保存。
 *
 * - 页面直接 v-model 读写 state，不再手写镜像 ref 与防抖保存逻辑
 * - state 与 Config 单例之间的同步：
 *   1. init / syncFromConfig：Config → state（全键拷贝）
 *   2. watch(state)：state → Config，只写入本次真正被修改的键（与上一份 state 快照求 diff），
 *      并防抖 commit 落盘
 *   3. watch(Config)：Config → state。外部修改配置文件后 'config-changed' 触发 Config.reload，
 *      reload 更新的是响应式 Config，这里把变化的键回拷到 state，页面驻留期间也能看到外部修改
 * - 为什么必须只写“真正变化的键”：若按 state 与 Config 的差异全量写回，外部修改过的键在 state
 *   里仍是旧值，用户随便改一项其他设置，就会把外部修改悄悄改回去并落盘
 */

import { reactive, ref, watch, type Ref } from 'vue';
import { dialog } from '../fluent-controls';

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
    /**
     * 上一次已与 Config 对齐的 state 快照。watch(state) 与它求 diff 得出用户本次修改的键；
     * Config → state 的回拷同时更新快照，所以回拷本身不会被当成用户修改而写回
     */
    const lastSyncedState = { ...CONFIG_DEFAULTS } as ConfigModel;

    /** 提交 config 落盘，失败时弹窗报错（文案沿用原设置页） */
    async function commitConfig(): Promise<void> {
        if (config == null) {
            return;
        }
        try {
            await config.commit();
        } catch (error) {
            console.error(error);
            await dialog.message(String(error), { title: '配置文件保存失败', kind: 'error' });
        }
    }

    /** 防抖自动保存：连续修改期间持续触发会重置计时，停止一段时间后自动保存 */
    const debouncedCommitConfig = debounce(() => void commitConfig(), COMMIT_DEBOUNCE_MS);

    /** Config → state 回拷单个键，并同步快照（避免被 watch(state) 当成用户修改写回） */
    function syncKeyFromConfig<K extends keyof ConfigModel>(source: Config, key: K): void {
        syncStoreKey(state, source, key);
        syncStoreKey(lastSyncedState, source, key);
    }

    function syncFromConfig(): void {
        if (config == null) {
            return;
        }
        for (const key of CONFIG_KEYS) {
            syncKeyFromConfig(config, key);
        }
    }

    /** 幂等初始化：并发/重复调用共享同一次执行 */
    let initPromise: Promise<void> | null = null;

    function init(): Promise<void> {
        if (initPromise == null) {
            initPromise = (async () => {
                config = await globals.getConfig();
                syncFromConfig();
                // state 的修改：与快照求 diff，只把用户本次改动的键写入 config（Config 的 setter
                // 自带脏标记），然后防抖保存；键都是原始值，无需 deep watch
                watch(state, () => {
                    if (config == null) {
                        return;
                    }
                    let changed = false;
                    for (const key of CONFIG_KEYS) {
                        if (state[key] !== lastSyncedState[key]) {
                            syncStoreKey(lastSyncedState, state, key);
                            syncStoreKey(config, state, key);
                            changed = true;
                        }
                    }
                    if (changed) {
                        debouncedCommitConfig();
                    }
                });
                // config 的变化（外部修改配置文件后的 reload，或其他页面直接改 config）：回拷到 state。
                // 自己刚写入 config 的键回拷的是同一个值，无副作用
                const cfg = config;
                watch(() => CONFIG_KEYS.map(key => cfg[key]), (newValues, oldValues) => {
                    CONFIG_KEYS.forEach((key, index) => {
                        if (newValues[index] !== oldValues[index]) {
                            syncKeyFromConfig(cfg, key);
                        }
                    });
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
