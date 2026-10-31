/**
 * 设置页各分区的“需要关注”聚合。
 *
 * 侧边栏导航项的徽标与主页设置按钮上的红点必须同源：否则会出现主界面按钮亮着、
 * 进设置却看不出是哪个分区有事（或反之）。两个入口都引用这里。
 *
 * - 关于：应用更新可用、Anki 内笔记模板有更新
 * - 划词：辅助功能权限未授权（macOS，且仅在已设置全局快捷键时——未设置时权限卡是禁用的，
 *   不该提示）、全局快捷键注册失败（冲突）
 */
import { computed } from 'vue';

import * as globals from './globals';
import { accessibilityTrusted, configuredShortcut, shortcutError } from './shortcut-status';
import type { SettingsPageId } from './settings-registry';

/** 某个设置分区待关注的项数（0 表示无需提示）。设置页导航用数字徽标展示 */
export function pageAttentionCount(pageId: SettingsPageId): number {
    if (pageId === 'about') {
        return (globals.appUpdateAvailable.value ? 1 : 0) + (globals.templateUpdateAvailable.value ? 1 : 0);
    }
    if (pageId === 'selection') {
        // 未设置快捷键时划词功能未启用，辅助功能权限卡处于禁用态，不算“需要关注”
        const shortcutSet = configuredShortcut.value.length > 0;
        return (shortcutSet && accessibilityTrusted.value === false ? 1 : 0) + (shortcutError.value != null ? 1 : 0);
    }
    return 0;
}

/** 设置里是否有需要关注的事项（主页设置按钮的红点） */
export const settingsNeedAttention = computed(
    () => pageAttentionCount('about') + pageAttentionCount('selection') > 0
);
