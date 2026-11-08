/**
 * 浏览器模式的桥接客户端：把 invoke 与事件接到 Rust 的 dev-bridge
 * （src-tauri/src/application/dev_bridge.rs，经 Vite 的 /__dev/bridge 代理，令牌不出现在浏览器里）。
 */
import { emit } from '@tauri-apps/api/event';

const BASE = '/__dev/bridge';
const HEADERS = { 'X-Dev-Bridge': '1' };

/** dev-bridge 是否在运行 */
export async function bridgeAvailable(): Promise<boolean> {
    try {
        const response = await fetch(`${BASE}/status`, { headers: HEADERS });
        return response.ok && ((await response.json()) as { available: boolean; }).available;
    } catch {
        return false;
    }
}

/** 转发一次 invoke，语义与 Tauri 原生 IPC 一致：JSON 参数或二进制参数；二进制响应返回 ArrayBuffer */
export async function bridgeInvoke(cmd: string, args: unknown, options?: { headers?: Record<string, string>; }): Promise<unknown> {
    const isRaw = args instanceof ArrayBuffer || ArrayBuffer.isView(args);
    const response = await fetch(`${BASE}/ipc/${encodeURIComponent(cmd)}`, {
        method: 'POST',
        headers: {
            ...HEADERS,
            ...options?.headers,
            'Content-Type': isRaw ? 'application/octet-stream' : 'application/json',
        },
        body: isRaw ? args as BodyInit : JSON.stringify(args ?? {}),
    });
    if (response.headers.get('x-ipc-error') === '1') {
        // 与原生一致：reject 的是 Rust 返回的错误值（通常是字符串）
        throw await response.json();
    }
    if (!response.ok) {
        throw new Error(`dev-bridge HTTP ${response.status}: ${await response.text()}`);
    }
    return response.headers.get('content-type')?.startsWith('application/json')
        ? response.json()
        : response.arrayBuffer();
}

/** 取应用日志文件的尾部（含前端 console 经 plugin:log 转发进来的内容） */
export async function bridgeLogs(options: { lines?: number; grep?: string; trace?: boolean; } = {}): Promise<string> {
    const query = new URLSearchParams();
    if (options.lines !== undefined) query.set('lines', String(options.lines));
    if (options.grep) query.set('grep', options.grep);
    if (options.trace) query.set('trace', '1');
    const response = await fetch(`${BASE}/logs?${query.toString()}`, { headers: HEADERS });
    return response.ok ? response.text() : `(no logs: HTTP ${response.status})`;
}

const subscribed = new Set<string>();

/** 告诉服务端开始转发某个事件（幂等） */
export function bridgeSubscribe(event: string): void {
    if (subscribed.has(event)) {
        return;
    }
    subscribed.add(event);
    void fetch(`${BASE}/subscribe/${encodeURIComponent(event)}`, { method: 'POST', headers: HEADERS })
        .catch(() => subscribed.delete(event));
}

/** 读 SSE 事件流，把 Rust 事件投递给本页的 listen（mockIPC 的本地事件系统）；断线后自动重连 */
export function startBridgeEvents(): void {
    const connect = async (): Promise<void> => {
        try {
            const response = await fetch(`${BASE}/events`, { headers: HEADERS });
            if (!response.ok || response.body == null) {
                throw new Error(`HTTP ${response.status}`);
            }
            // 重连后服务端的订阅仍在，但断线期间的事件已丢失，属于可接受的 dev 行为
            const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
            let buffer = '';
            for (;;) {
                const { value, done } = await reader.read();
                if (done) break;
                buffer += value;
                let index: number;
                while ((index = buffer.indexOf('\n\n')) >= 0) {
                    const chunk = buffer.slice(0, index);
                    buffer = buffer.slice(index + 2);
                    const data = chunk.split('\n').find(line => line.startsWith('data: '));
                    if (data != null) {
                        const message = JSON.parse(data.slice(6)) as { event: string; payload: unknown; };
                        void emit(message.event, message.payload);
                    }
                }
            }
        } catch (error) {
            console.warn('[dev-bridge] 事件流中断，2 秒后重连', error);
        }
        setTimeout(() => void connect(), 2000);
    };
    void connect();
}
