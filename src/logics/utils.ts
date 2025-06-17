import * as api from '../tauri-api';

export * as string from './stringutils';
export { isWord, tokenize, escapeHTML } from './stringutils';
export * as typing from './typing';

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    try {
        return await api.core.invoke<T>(cmd, args);
    } catch (error) {
        throw (typeof error === 'string') ? new Error(error) : error;
    }
}

/**
 * 检测是否在 release 模式下
 * 
 * https://github.com/tauri-apps/wry/issues/30
 * 
 * 该方法在 Linux 上可能无法正常工作。
 */
export function tauriInRelease(): boolean {
    return window.location.hostname === 'tauri.localhost';
}

export async function rustInRelease(): Promise<boolean> {
    return await invoke<boolean>('rust_in_release');
}

export async function sanitizeFilename(filename: string): Promise<string> {
    return await invoke<string>('sanitize_filename', { filename });
}

/**
 * 创建一个防抖函数
 * 
 * @param func 要延迟执行的函数
 * @param wait 延迟时间（毫秒）
 * @param immediate 是否立即执行，若为 `false` 则为标准的防抖逻辑，延迟 `wait` 毫秒后执行 `func`；
 * 若为 `true` 则立即执行函数 `func`，然后在 `wait` 毫秒内不再执行。
 * @returns 返回一个防抖函数
 */
export function debounce<F extends (...args: any[]) => any>(
    func: F,
    wait: number,
    immediate: boolean = false
): (...args: Parameters<F>) => void {
    let timeout: ReturnType<typeof setTimeout> | null = null;

    return function (this: any, ...args: Parameters<F>) {
        if (timeout != null) {
            // 若延迟定时器已被设置，则清除之前的定时器，重新设置一个延迟定时器
            clearTimeout(timeout);
        }
        const callNow = immediate && (timeout == null); // 需要在设置 timeout 前判断 timeout 是否为 null
        timeout = setTimeout(() => {
            timeout = null;
            if (!immediate) {
                func.apply(this, args); // 延迟执行的防抖逻辑
            }
        }, wait);
        if (callNow) {
            func.apply(this, args); // 立即执行的防抖逻辑
        }
    };
}

/**
 * 创建一个节流函数
 * 
 * @param func 要被限制执行频率的函数
 * @param wait 时间间隔（毫秒）
 * @returns 返回一个节流函数
 */
export function throttle<F extends (...args: any[]) => any>(func: F, wait: number): (...args: Parameters<F>) => void {
    let timeout: number | null = null;
    let lastExec = 0;

    return function (this: any, ...args: Parameters<F>): void {
        const now = Date.now();

        const execute = () => {
            lastExec = now;
            func.apply(this, args);
        };

        if (timeout != null) {
            clearTimeout(timeout);
        }
        if (now - lastExec >= wait) {
            execute();
        } else {
            // ensure the last call will be executed
            timeout = setTimeout(execute, wait - (now - lastExec));
        }
    };
}

/** 将 HEX 转换为 RGB */
export function hexToRgb(hex: string): { r: number; g: number; b: number; } {
    const bigint: number = parseInt(hex.slice(1), 16);
    const r: number = (bigint >> 16) & 255;
    const g: number = (bigint >> 8) & 255;
    const b: number = bigint & 255;
    return { r, g, b };
}

/** 将 RGB 转换为 HSL */
export function rgbToHsl(r: number, g: number, b: number): { h: number; s: number; l: number; } {
    r /= 255;
    g /= 255;
    b /= 255;

    const max: number = Math.max(r, g, b);
    const min: number = Math.min(r, g, b);
    let h: number = 0, s: number = 0;
    const l: number = (max + min) / 2;

    if (max !== min) {
        const d: number = max - min;
        s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
        switch (max) {
            case r: h = (g - b) / d + (g < b ? 6 : 0); break;
            case g: h = (b - r) / d + 2; break;
            case b: h = (r - g) / d + 4; break;
        }
        h /= 6;
    }

    return { h: Math.round(h * 360), s: Math.round(s * 100), l: Math.round(l * 100) };
}

export function disableWebviewContextMenu(): void {
    document.addEventListener('contextmenu', e => {
        if (!((e.target instanceof HTMLInputElement && e.target.type === 'text')
            || e.target instanceof HTMLTextAreaElement)) {
            e.preventDefault();
        }
    });
}

/**
 * Disable WebView keyboard shortcuts.
 * 
 * See https://support.google.com/chrome/answer/157179
 */
export function disableWebviewKeyboardShortcuts(): void {
    document.addEventListener("keydown", e => {
        if (e.key === 'F5' || (e.ctrlKey && e.key === 'r')) { // 禁用 F5 或 Ctrl + R 刷新
            e.preventDefault();
        }
        if (e.key === 'F3' || (e.ctrlKey && e.key === 'f')) { // 禁用 F3 或 Ctrl + F 查找
            e.preventDefault();
        }
        if (e.ctrlKey && e.key === 'j') { // 禁用 Ctrl + J 打开下载内容
            e.preventDefault();
        }
        if (e.ctrlKey && e.key === 'h') { // 禁用 Ctrl + H 打开历史记录
            e.preventDefault();
        }
        if (e.key === 'F7') { // 禁用 F7 开启光标浏览模式
            e.preventDefault();
        }
        if (e.ctrlKey && e.key === 'g') { // 禁用 Ctrl + G 查找下一个
            e.preventDefault();
        }
        if (e.ctrlKey && e.key === 'p') { // 禁用 Ctrl + P 打印
            e.preventDefault();
        }
    });
}
