/**
 * 网络请求的统一入口。
 *
 * 业务代码不直接使用 @tauri-apps/plugin-http 的 fetch：dev 功能需要在真实 Rust 网络栈
 * 之前截获请求，才能在原生 dev 窗口与浏览器模式里用同一套夹具预览 AI、Anki 等行为。
 * 总开关关闭（含 release）时这里只做一次编译期可消除的判断，随后直接走真实网络。
 */
import { fetch as tauriFetch } from '@tauri-apps/plugin-http';

/** dev 夹具的 HTTP 拦截器；返回 undefined 表示不处理，交回真实网络栈 */
export type DevFetchInterceptor = (
    input: string | URL | Request,
    init?: RequestInit,
) => Response | undefined | Promise<Response | undefined>;

export async function fetch(input: string | URL | Request, init?: RequestInit): Promise<Response> {
    if (__DEV_FEATURES__) {
        const interceptor = (window as unknown as { __devFetchInterceptor?: DevFetchInterceptor })
            .__devFetchInterceptor;
        if (interceptor != null) {
            const response = await interceptor(input, init);
            if (response !== undefined) {
                return response;
            }
        }
    }
    return await tauriFetch(input, init);
}
