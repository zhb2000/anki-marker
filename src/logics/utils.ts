import * as api from '@tauri-apps/api';

export * as string from './stringutils';
export { isWord, tokenize, escapeHTML } from './stringutils';
export * as typing from './typing';

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    try {
        return await api.tauri.invoke<T>(cmd, args);
    } catch (error) {
        throw (typeof error === 'string') ? new Error(error) : error;
    }
}

/**
 * 检测是否在 release 模式下
 * 
 * https://github.com/tauri-apps/wry/issues/30
 */
export function tauriInRelease(): boolean {
    return window.location.hostname === 'tauri.localhost';
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
    let h: number = 0, s: number = 0, l: number = (max + min) / 2;

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
