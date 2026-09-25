import { createApp, reactive } from 'vue';
import ContentDialogHost from './ContentDialogHost.vue';
import type { FluentDialogCommand } from './FluentDialogShell.vue';

/**
 * WinUI ContentDialog 风格的命令式消息/确认弹窗，替代 @tauri-apps/plugin-dialog
 * 的系统原生 message/confirm（后者在三端呈现 Windows 任务对话框 / macOS NSAlert /
 * Linux GTK 对话框三种互不相同的视觉，与全应用统一的 Fluent 设计语言脱节）。
 *
 * 用法与 plugin-dialog 的 message/confirm 保持签名兼容，调用点只需更换导入来源：
 *   await dialog.message(String(error), { title: '查询失败', kind: 'error' });
 *   const ok = await dialog.confirm('…', { title: '…', kind: 'warning', okLabel: '…' });
 *
 * kind 仅有两级：error（操作失败类消息）与 warning（破坏性操作确认）；
 * 不传则纯文字无图标。图标展示在标题左侧，正文始终是纯文字。
 *
 * 实现：单例宿主组件挂载到 body 下，通过共享的 reactive 状态驱动渲染；
 * 并发请求进入队列串行展示（后一个等前一个退场动画播完再出现）。
 */

/** 消息级别：决定标题左侧的图标与颜色。不传（undefined）则纯文字无图标 */
export type ContentDialogKind = 'warning' | 'error';

export interface ContentDialogMessageOptions {
    title?: string;
    kind?: ContentDialogKind;
    /** 确认按钮文案，默认“确定” */
    okLabel?: string;
}

export interface ContentDialogConfirmOptions extends ContentDialogMessageOptions {
    /** 取消按钮文案，默认“取消” */
    cancelLabel?: string;
}

interface ContentDialogRequest {
    title: string;
    message: string;
    /** 不传则无图标（默认） */
    kind?: ContentDialogKind;
    /** 底部命令区按钮（ok 居左为默认按钮，cancel 居右，符合 ContentDialog Primary/Close 布局） */
    commands: FluentDialogCommand[];
    /** 以被点击的命令 key 结算；Esc/点遮罩时为 null */
    resolve: (key: string | null) => void;
}

/** 传给宿主组件的共享状态 */
export interface ContentDialogHostState {
    open: boolean;
    request: ContentDialogRequest | null;
}

const hostState = reactive<ContentDialogHostState>({ open: false, request: null });
const queue: ContentDialogRequest[] = [];
let hostContainer: HTMLElement | null = null;

/** 对话框开/关回调：展示开始时为 true，结算（关闭）时为 false */
type DialogOpenListener = (open: boolean) => void;
let dialogOpenListener: DialogOpenListener | null = null;

/**
 * 注册对话框开/关回调（重复注册覆盖前一个，传 null 注销）。
 * fluent-controls 不依赖 Tauri 等宿主环境，需要随对话框开/关联动的副作用
 * （如 macOS 标题栏随遮罩变色）由宿主经此钩子接入。
 */
export function setDialogOpenListener(listener: DialogOpenListener | null): void {
    dialogOpenListener = listener;
}

/** 首次使用时创建宿主容器并挂载（此后复用同一实例） */
function ensureHost(): void {
    if (hostContainer != null) {
        return;
    }
    hostContainer = document.createElement('div');
    document.body.appendChild(hostContainer);
    createApp(ContentDialogHost, {
        host: hostState,
        onClose: () => settle(null),
        onCommand: (key: string) => settle(key),
    }).mount(hostContainer);
}

function dequeue(): void {
    const next = queue.shift();
    if (next == null) {
        return;
    }
    hostState.request = next;
    hostState.open = true;
    dialogOpenListener?.(true);
}

/**
 * 结算当前弹窗：立即 resolve 并开始退场动画；等动画播完（外壳 leave 过渡最长 0.25s）
 * 再清空请求并展示队列中的下一个。
 */
function settle(key: string | null): void {
    const current = hostState.request;
    if (current == null) {
        return;
    }
    hostState.open = false;
    dialogOpenListener?.(false);
    current.resolve(key);
    window.setTimeout(() => {
        hostState.request = null;
        dequeue();
    }, 260);
}

function openDialog(request: ContentDialogRequest): Promise<string | null> {
    ensureHost();
    return new Promise(resolve => {
        queue.push({ ...request, resolve });
        // 无弹窗展示中（且没有正在退场的请求）时立即展示，否则等 settle 里播完动画后接管
        if (!hostState.open && hostState.request == null) {
            dequeue();
        }
    });
}

/** 消息弹窗：单个确认按钮，resolve 表示用户已确认 */
export function showMessage(message: string, options: ContentDialogMessageOptions = {}): Promise<void> {
    return openDialog({
        title: options.title ?? '提示',
        message,
        kind: options.kind,
        commands: [{ key: 'ok', label: options.okLabel ?? '确定', accent: true }],
        resolve: () => undefined,
    }).then(() => undefined);
}

/** 确认弹窗：resolve 为 true 表示点击了确认按钮；Esc/遮罩/取消均为 false */
export function showConfirm(message: string, options: ContentDialogConfirmOptions = {}): Promise<boolean> {
    return openDialog({
        title: options.title ?? '确认',
        message,
        kind: options.kind,
        commands: [
            { key: 'ok', label: options.okLabel ?? '确定', accent: true },
            { key: 'cancel', label: options.cancelLabel ?? '取消' },
        ],
        resolve: () => undefined,
    }).then(key => key === 'ok');
}

/** 聚合导出：调用点以 dialog.message / dialog.confirm 的形式使用，与原 api.dialog 同形 */
export const dialog = {
    message: showMessage,
    confirm: showConfirm,
};
