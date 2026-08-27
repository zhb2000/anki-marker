// 全局快捷键与辅助功能权限的全局状态（设置页改进 #6，并为 #7 导航红点提供状态）。
//
// 状态在应用启动时初始化一次并全局常驻：
// - 'shortcut-registration' 事件监听全局常驻（原由 SettingsView 持有，设置页关闭后失效）；
// - 窗口焦点监听全局常驻、不注销——权限状态需在任何时刻（含未打开设置页时）
//   从系统设置授权返回后自动刷新，供导航红点使用。
import { ref, type Ref } from 'vue';
import { ElMessage } from 'element-plus';

import * as api from '../tauri-api';
import { invoke } from './utils';

/** 快捷键注册结果（Rust 侧在注册/注销后 emit，亦可由 get_shortcut_registration 主动查询） */
interface ShortcutRegistration {
    shortcut: string;
    success: boolean;
    error: string | null;
}

/** 最近一次全局快捷键注册结果的错误信息；null 表示正常（或未设置快捷键） */
export const shortcutError: Ref<string | null> = ref(null);

/** 辅助功能权限状态：null 表示尚未检查 */
export const accessibilityTrusted: Ref<boolean | null> = ref(null);

/** 是否为 macOS（全局快捷键与辅助功能权限目前仅支持 macOS） */
const isMacOS = api.os.type() === 'macos';

/** 是否已经初始化过（幂等） */
let initialized = false;

/**
 * 幂等初始化：
 * 1. 挂 'shortcut-registration' 事件监听（全局常驻，不注销）；
 * 2. 主动查询一次最近的注册结果，补齐前端尚未就绪而错过的 emit；
 * 3. macOS 上立即检查一次辅助功能权限，并挂窗口焦点监听（全局常驻，不注销）。
 */
export async function initShortcutStatus(): Promise<void> {
    if (initialized) {
        return;
    }
    initialized = true;
    // 监听快捷键注册结果事件，失败时记录错误并向用户反馈（文案与原 SettingsView 一致）
    try {
        await api.event.listen<ShortcutRegistration>('shortcut-registration', event => {
            applyRegistration(event.payload, true);
        });
    } catch (error) {
        console.error(error);
    }
    // 主动查询一次：应用启动时前端尚未挂监听，启动注册的结果事件可能已错过；
    // 返回 null 表示尚无记录（如非 macOS 平台），不视为错误，保持现状
    try {
        const registration = await invoke<ShortcutRegistration | null>('get_shortcut_registration');
        if (registration != null) {
            applyRegistration(registration, false);
        }
    } catch (error) {
        console.error(error);
    }
    if (isMacOS) {
        // 首次检查一次辅助功能权限状态
        void checkAccessibilityTrust();
        // 从其他应用切回本应用时（如从系统设置授权后返回）自动刷新权限状态。
        // 全局常驻不注销：与页面生命周期解耦，保证导航红点状态始终最新
        try {
            await api.window.getCurrentWindow().onFocusChanged(({ payload: focused }) => {
                if (focused) {
                    void checkAccessibilityTrust();
                }
            });
        } catch (error) {
            console.error(error);
        }
    }
}

/** 应用一次注册结果：成功时清除错误；失败时记录错误（notify 为 true 时弹错提示） */
function applyRegistration(registration: ShortcutRegistration, notify: boolean) {
    if (registration.success) {
        shortcutError.value = null;
    } else {
        shortcutError.value = registration.error ?? '未知错误';
        if (notify) {
            ElMessage.error(`全局快捷键注册失败：${shortcutError.value}`);
        }
    }
}

/** 检查辅助功能权限状态 */
export async function checkAccessibilityTrust(): Promise<void> {
    if (!isMacOS) {
        return;
    }
    try {
        accessibilityTrusted.value = await invoke<boolean>('is_accessibility_trusted');
    } catch (error) {
        console.error(error);
    }
}

/** 申请辅助功能权限：弹出系统授权弹窗；若弹窗曾被拒绝则直接打开系统设置的辅助功能面板 */
export async function requestAccessibilityTrust(): Promise<void> {
    try {
        await invoke('request_accessibility_trust');
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '申请辅助功能权限失败', kind: 'error' });
        return;
    }
    // 授权完成后切回本应用时，由窗口焦点监听自动刷新状态
}
