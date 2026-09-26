<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';

import * as api from '../../tauri-api';
import { FluentHyperlink, FluentSelect, FluentSettingCard, FluentToggleSwitch, type FluentSelectOption } from '../../fluent-controls';
import { ResetButton } from '../../components';
import { useSettingsStore } from '../../logics/settings-store';
import * as cfg from '../../logics/config';
import * as debug from '../../logics/debug';
import * as utils from '../../logics/utils';
import { setThemeMode } from '../../logics/theme';
import { useHighlight } from './useHighlight';

const store = useSettingsStore();
const state = store.state;

// 搜索跳转高亮（见 useHighlight 注释）
useHighlight();

/** 主题下拉的选项：跟随系统 / 浅色 / 深色 */
const themeOptions: FluentSelectOption[] = [
    { value: 'system', label: '跟随系统' },
    { value: 'light', label: '浅色' },
    { value: 'dark', label: '深色' },
];

/** 是否为 macOS（后台图标选项与部分文案有平台差异） */
const isMacOS = computed(() => api.os.type() === 'macos');

/** 后台运行图标下拉的选项：macOS 为 Dock 栏图标/菜单栏图标/不显示图标；其他平台仅有托盘/不显示图标 */
const backgroundIconOptions = computed<FluentSelectOption[]>(() => isMacOS.value
    ? [
        { value: 'menu-bar', label: '菜单栏图标' },
          { value: 'dock', label: 'Dock 栏图标' },
          { value: 'none', label: '不显示图标' },
      ]
    : [
          { value: 'menu-bar', label: '托盘图标' },
          { value: 'none', label: '不显示图标' },
      ]);

// 主题切换即时生效：全窗口共享同一 DOM，setThemeMode 直接切换 dark class，无需等待保存
watch(() => state.theme, theme => setThemeMode(theme));

/** 后台图标设置未启用时禁用：卡片常显（搜索跳转锚点始终有效、布局稳定），仅禁用交互 */
const backgroundIconDisabled = computed(() => !state.keepRunningOnClose);

/** 后台图标卡的说明文案：功能关闭时替换为禁用原因 */
const backgroundIconDescription = computed(() => backgroundIconDisabled.value
    ? '需先开启“关闭窗口后保持后台运行”'
    : isMacOS.value
        ? '选择窗口关闭后（后台运行期间）应用图标的显示位置'
        : '选择窗口关闭后（后台运行期间）是否显示托盘图标'
);

/** “关闭窗口后保持后台运行”卡的说明文案：唤起入口按平台区分 */
const keepRunningOnCloseDescription = computed(() => isMacOS.value
    ? '关闭窗口后应用将在后台继续运行，可通过 Dock 图标、菜单栏图标或全局快捷键再次打开'
    : '关闭窗口后应用将在后台继续运行，可通过托盘图标再次打开'
);

// 登录时自动启动：状态存于系统（macOS 为登录项，Windows 为注册表 Run 键，Linux 为
// XDG Autostart），不属于 config，不进设置仓库（也因此无 ResetButton），以系统实查为准。
const launchAtLogin = ref(false);
/** 初始系统状态是否已查询完成：完成前忽略用户切换，避免用未知旧状态覆盖真实状态 */
const launchAtLoginLoaded = ref(false);
/** 正在向系统提交切换：提交期间忽略后续切换，防止竞态 */
const launchAtLoginSyncing = ref(false);
/**
 * 当前 Rust 侧是否为正式构建（null 表示尚未查询完成）。
 * dev 构建下禁用自启动修改：dev 与生产安装读同一份系统注册（Linux 的 .desktop、
 * Windows 的注册表 Run 值均按 productName 命名，macOS 登录项同名），dev 下开一次
 * 开关就会把 target/debug 二进制注册为自启动、覆盖生产注册，且该二进制登录拉起时
 * 没有 dev server 可连，必然异常。
 */
const rustInRelease = ref<boolean | null>(null);
/**
 * dev 构建下锁定修改（沿用 backgroundIconDisabled 模式：卡片常显保搜索锚点，仅禁用交互）。
 * 需要查看开启状态的界面效果时，可在 `.env.development.local` 配置
 * `VITE_LAUNCH_AT_LOGIN_EDITABLE=true` 解锁（见 debug.launchAtLoginEditableInDev）。
 */
const launchAtLoginLocked = computed(() =>
    rustInRelease.value === false && !debug.launchAtLoginEditableInDev
);

/** 登录自启动卡的说明文案：三端均静默常驻后台，唤起入口按平台区分 */
const launchAtLoginDescription = computed(() => isMacOS.value
    ? '登录系统后自动启动 Anki Marker 并在后台常驻，可通过菜单栏图标、Dock 图标或全局快捷键打开'
    : '登录系统后自动启动 Anki Marker 并在后台常驻，可通过托盘图标打开'
);

/**
 * 系统登录项管理面板的入口（按平台）：
 * macOS 直跳“系统设置 › 登录项”，Windows 跳“设置 › 应用 › 启动”；
 * Linux 无统一管理面板，返回 null 不显示入口。
 * 点击走 open_in_browser——`open`/`start` 对自定义 scheme 的处理天然覆盖这两种 URI。
 * 入口定位是低频逃生通道（用户在系统面板中误关/误删后自行恢复），故用描述内
 * 超链接而非按钮，不与卡片主控件（开关）争夺视觉权重。
 */
const loginItemsPanel = computed(() => {
    const os = api.os.type();
    if (os === 'macos') {
        return { text: '系统设置 › 登录项', url: 'x-apple.systempreferences:com.apple.LoginItems-Settings.extension' };
    }
    if (os === 'windows') {
        return { text: '系统启动应用设置', url: 'ms-settings:startupapps' };
    }
    return null;
});

onMounted(async () => {
    try {
        launchAtLogin.value = await api.autostart.isEnabled();
    } catch (error) {
        console.warn('[settings] failed to query launch-at-login state:', error);
    } finally {
        launchAtLoginLoaded.value = true;
    }
    try {
        rustInRelease.value = await utils.rustInRelease();
    } catch (error) {
        // 查询失败不锁定（保持 null），与未引入锁定前的行为一致：锁定只是 dev 便利保护，
        // 无需为瞬时故障牺牲正式环境下的可用性
        console.warn('[settings] failed to query rust build type:', error);
    }
});

/** 切换登录自启动：无论提交成败，最终都回读系统实查状态同步 UI（含失败回滚） */
async function onLaunchAtLoginChange(enabled: boolean | undefined) {
    if (enabled === undefined) return;
    if (!launchAtLoginLoaded.value || launchAtLoginSyncing.value) return;
    launchAtLoginSyncing.value = true;
    try {
        if (enabled) {
            await api.autostart.enable();
        } else {
            await api.autostart.disable();
        }
    } catch (error) {
        console.warn(`[settings] failed to ${enabled ? 'enable' : 'disable'} launch at login:`, error);
    } finally {
        try {
            launchAtLogin.value = await api.autostart.isEnabled();
        } catch {
            // 回读失败：保留提交前的乐观状态
        } finally {
            launchAtLoginSyncing.value = false;
        }
    }
}
</script>

<template>
    <div class="settings-page">
        <h2 class="group-title">外观</h2>
        <div class="card-list">
            <FluentSettingCard header="主题" setting-id="theme">
                <template #header-extra>
                    <ResetButton setting-key="theme" />
                </template>
                <FluentSelect :options="themeOptions" v-model="state.theme" />
            </FluentSettingCard>
        </div>

        <h2 class="group-title">启动</h2>
        <div class="card-list">
            <FluentSettingCard header="登录时自动启动" setting-id="launchAtLogin" :disabled="launchAtLoginLocked">
                <template #description>
                    <template v-if="launchAtLoginLocked">开发模式下不可修改，请使用打包安装的版本测试自启动</template>
                    <template v-else>
                        {{ launchAtLoginDescription }}<template v-if="loginItemsPanel">；也可在<FluentHyperlink
                            :title="'打开 ' + loginItemsPanel.text"
                            @click="cfg.openInBrowser(loginItemsPanel.url)">{{ loginItemsPanel.text }}</FluentHyperlink>中管理</template>
                    </template>
                </template>
                <FluentToggleSwitch :model-value="launchAtLogin" :disabled="launchAtLoginLocked"
                    @update:model-value="onLaunchAtLoginChange" />
            </FluentSettingCard>
        </div>

        <h2 class="group-title">窗口</h2>
        <div class="card-list">
            <FluentSettingCard header="关闭窗口后保持后台运行" :description="keepRunningOnCloseDescription"
                setting-id="keepRunningOnClose">
                <template #header-extra>
                    <ResetButton setting-key="keepRunningOnClose" />
                </template>
                <FluentToggleSwitch v-model="state.keepRunningOnClose" />
            </FluentSettingCard>
            <FluentSettingCard header="后台运行时显示图标" :description="backgroundIconDescription"
                setting-id="backgroundIcon" :disabled="backgroundIconDisabled">
                <template #header-extra>
                    <ResetButton setting-key="backgroundIcon" :disabled="backgroundIconDisabled" />
                </template>
                <FluentSelect :options="backgroundIconOptions" v-model="state.backgroundIcon"
                    :disabled="backgroundIconDisabled" />
            </FluentSettingCard>
        </div>
    </div>
</template>

<style scoped>
.group-title {
    margin: 24px 0 6px 2px;
    padding: 0;
    font-size: 14px;
    font-weight: normal;
    opacity: 0.6;
    user-select: none;
    cursor: default;
}

.group-title:first-child {
    margin-top: 0;
}

.card-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
}
</style>
