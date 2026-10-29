/**
 * 弹窗层级、遮罩联动与最上层判定。
 *
 * 所有 FluentDialogShell 打开/关闭都经这里登记，不再由某个具体弹窗实现各自通知：
 * - 遮罩：每次栈深度变化都通知宿主当前层数。宿主据此同步 macOS 标题栏的遮罩色
 *   （见 window.rs 的 set_dialog_mask）：页面上的每层对话框各叠一层 smoke 遮罩，
 *   标题栏按同一公式逐层加深——只上报“有没有弹窗”会让嵌套确认框出现时标题栏停住，
 *   而页面已经又暗了一层。
 * - 层级：后打开的压在前一个之上。Teleport 的 DOM 顺序取决于挂载时间而不是打开时间，
 *   不显式分层时后挂载的弹窗会盖住晚打开的命令式提示。
 * - 最上层：Esc 只关闭最上层，避免嵌套时一次按键关掉两个弹窗（外壳的 keydown 监听
 *   挂在 window 上，所有打开的弹窗都会收到）。
 */

/** 弹窗在栈中的身份与层级 */
export interface DialogLayer {
    id: number;
    zIndex: number;
}

/** 宿主关注的“当前打开了几层弹窗”回调（0 表示没有弹窗） */
type DialogDepthListener = (depth: number) => void;

let depthListener: DialogDepthListener | null = null;
/** 当前打开的弹窗栈（按打开顺序，末尾即最上层） */
const stack: DialogLayer[] = [];
let nextId = 0;
/** 层级游标：只增不减，保证“后打开在上” */
let zIndexSeed = 1000;

/** 注册“当前弹窗层数”的回调（重复注册覆盖前一个，传 null 注销） */
export function setDialogDepthListener(listener: DialogDepthListener | null): void {
    depthListener = listener;
}

/** 登记一个刚打开的弹窗，返回其身份与层级 */
export function enterDialog(): DialogLayer {
    nextId += 1;
    zIndexSeed += 1;
    const layer: DialogLayer = { id: nextId, zIndex: zIndexSeed };
    stack.push(layer);
    depthListener?.(stack.length);
    return layer;
}

/** 登记一个弹窗关闭 */
export function leaveDialog(id: number): void {
    const index = stack.findIndex(layer => layer.id === id);
    if (index < 0) {
        return;
    }
    stack.splice(index, 1);
    depthListener?.(stack.length);
}

/** 该弹窗是否为当前最上层：Esc 只应由最上层响应 */
export function isTopDialog(id: number): boolean {
    return stack.length > 0 && stack[stack.length - 1].id === id;
}