import { invoke as tauriInvoke } from '@tauri-apps/api/tauri';

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    try {
        return await tauriInvoke<T>(cmd, args);
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
