/**
 * dev 设置的响应式状态。业务代码不要直接读它，统一经 logics/debug.ts（那里负责
 * “release 构建恒为真实行为”）；写入只发生在 dev/store.ts。
 */
import { reactive } from 'vue';

import { defaultDevSettings, type DevSettingKey, type DevSettings } from './schema';

/** 当前生效的 dev 设置（已合并 URL/会话覆盖、文件值与默认值） */
export const devSettings: DevSettings = reactive(
    __DEV_FEATURES__ ? defaultDevSettings() : {} as DevSettings
);

/** 被 URL 参数或 `window.__dev.set` 覆盖的键（仅当前页面会话有效，不写入文件） */
export const devOverridden = reactive<Partial<Record<DevSettingKey, true>>>({});
