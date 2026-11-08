/**
 * 浏览器模式的网络夹具：应用的 HTTP 请求都走 @tauri-apps/plugin-http（Rust 侧网络栈），
 * 这里在它的 IPC 层拦截，业务代码无需任何改动。
 *
 * 命中的目标：AnkiConnect（按“Anki 状态”设置返回正常结果或连接失败）、有道词典、
 * GitHub Release、OpenAI 兼容服务的模型列表。LLM 的聊天补全（SSE）由
 * src/dev/mock/llm.ts 在更前面的前端 HTTP 入口拦截，不经过这里；这里不再重复实现。
 */
import pkg from '../../../package.json';
import MODEL_FRONT from '../../assets/model-front.html?raw';
import MODEL_BACK from '../../assets/model-back.html?raw';
import { devSettings } from '../state';
import { youdaoXml } from './fixtures';
import { ankiRunning } from './runtime';

export interface MockRequest {
    method: string;
    url: URL;
    headers: Headers;
    body: string;
}

export interface MockResponse {
    status?: number;
    headers?: Record<string, string>;
    body: string;
}

const MODEL_NAME_FALLBACK = '划词助手默认单词模板';

/** 无法连接时模拟 reqwest 的报错（plugin-http 在连接失败时 reject 一个字符串） */
function connectionError(url: URL): never {
    throw new Error(`error sending request for url (${url.href}): connection refused (dev mock)`);
}

let nextNoteId = Date.now();

function ankiConnect(request: MockRequest): MockResponse {
    if (!ankiRunning()) {
        connectionError(request.url);
    }
    if (devSettings.ankiState === 'plugin-missing') {
        connectionError(request.url);
    }
    const { action, params } = JSON.parse(request.body) as { action: string; params?: Record<string, any>; };
    let result: unknown;
    switch (action) {
        case 'version': result = 6; break;
        case 'deckNames': result = ['Default', '划词助手默认牌组']; break;
        case 'deckNamesAndIds': result = { Default: 1, '划词助手默认牌组': 2 }; break;
        case 'modelNames': result = [MODEL_NAME_FALLBACK]; break;
        case 'findModelsByName':
            result = (params?.modelNames as string[] ?? []).map(name => ({
                name,
                tmpls: [{ name: 'Card 1', ord: 0, qfmt: MODEL_FRONT, afmt: MODEL_BACK }],
            }));
            break;
        case 'addNote': result = nextNoteId++; break;
        case 'findNotes': result = []; break;
        case 'notesInfo': result = []; break;
        case 'cardsInfo': result = []; break;
        case 'guiBrowse': result = []; break;
        default: result = null;
    }
    return { body: JSON.stringify({ result, error: null }) };
}

function handle(request: MockRequest): MockResponse {
    const { url } = request;
    if (url.port === '8765' || request.method === 'POST' && request.body.includes('"version":6')) {
        return ankiConnect(request);
    }
    if (url.hostname === 'dict.youdao.com' && url.pathname === '/fsearch') {
        return { headers: { 'content-type': 'text/xml' }, body: youdaoXml(url.searchParams.get('q') ?? '') };
    }
    if (url.hostname === 'api.github.com') {
        // 与当前版本相同 = “已是最新”；需要“有新版本/请求失败”请用“应用更新检查”场景
        return {
            headers: { 'content-type': 'application/json', etag: 'W/"dev-mock"' },
            body: JSON.stringify({
                tag_name: `v${pkg.version}`,
                html_url: `https://github.com/zhb2000/anki-marker/releases/tag/v${pkg.version}`,
                name: `Anki Marker v${pkg.version}`,
                body: '',
            }),
        };
    }
    if (request.method === 'GET' && url.pathname.endsWith('/models')) {
        return {
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ data: [{ id: 'mock-model-a', owned_by: 'dev' }, { id: 'mock-model-b' }] }),
        };
    }
    connectionError(url);
}

interface PendingRequest { request: MockRequest; }
interface PendingResponse { bytes: Uint8Array; read: boolean; }

const requests = new Map<number, PendingRequest>();
const responses = new Map<number, PendingResponse>();
/** 夹具自己分配的 rid 从一个很大的数开始，与桥接转发回来的真实 rid 不会撞 */
let nextRid = 1_000_000_000;

/** 交给调用方继续处理（桥接模式下转发给真实 Rust）的标记 */
export const PASSTHROUGH = Symbol('passthrough');

/** 是否是发往 AnkiConnect 的请求（默认端口 8765，或 AnkiConnect 的 JSON 协议体） */
function isAnkiConnectRequest(config: { method: string; url: string; data: number[] | null; }): boolean {
    const url = new URL(config.url);
    return url.port === '8765' || (config.method === 'POST' && config.data != null
        && new TextDecoder().decode(new Uint8Array(config.data)).includes('"version":6'));
}

/**
 * plugin:http 的 IPC 命令。
 * - 夹具模式（passthrough=false）：全部由夹具响应；
 * - 桥接模式（passthrough=true）：只有 AnkiConnect 请求仍由夹具响应（除非“Anki 状态”选了真实 Anki，
 *   避免调试时误写真实牌组），其余真实联网，返回 PASSTHROUGH 让调用方转发给 Rust。
 */
export async function handleHttpCommand(cmd: string, args: Record<string, any>, passthrough = false): Promise<unknown> {
    switch (cmd) {
        case 'plugin:http|fetch': {
            const config = args.clientConfig as { method: string; url: string; headers: [string, string][]; data: number[] | null; };
            if (passthrough && (devSettings.ankiState === 'real' || !isAnkiConnectRequest(config))) {
                return PASSTHROUGH;
            }
            const rid = nextRid++;
            requests.set(rid, {
                request: {
                    method: config.method,
                    url: new URL(config.url),
                    headers: new Headers(config.headers),
                    body: config.data == null ? '' : new TextDecoder().decode(new Uint8Array(config.data)),
                },
            });
            return rid;
        }
        case 'plugin:http|fetch_send': {
            if (passthrough && !requests.has(args.rid as number)) {
                return PASSTHROUGH;
            }
            const pending = requests.get(args.rid as number);
            requests.delete(args.rid as number);
            if (pending == null) {
                throw new Error('Request cancelled');
            }
            await new Promise(resolve => setTimeout(resolve, 80)); // 模拟最小的网络耗时
            const response = handle(pending.request);
            const rid = nextRid++;
            responses.set(rid, { bytes: new TextEncoder().encode(response.body), read: false });
            const status = response.status ?? 200;
            return {
                status,
                statusText: status === 200 ? 'OK' : '',
                url: pending.request.url.href,
                headers: Object.entries(response.headers ?? { 'content-type': 'application/json' }),
                rid,
            };
        }
        case 'plugin:http|fetch_read_body': {
            if (passthrough && !responses.has(args.rid as number)) {
                return PASSTHROUGH;
            }
            const pending = responses.get(args.rid as number);
            if (pending == null || pending.read) {
                return [1]; // 末字节 1 表示流结束
            }
            pending.read = true;
            return [...pending.bytes, 0];
        }
        case 'plugin:http|fetch_cancel':
        case 'plugin:http|fetch_cancel_body':
            if (passthrough && Number(args.rid) < 1_000_000_000) {
                return PASSTHROUGH;
            }
            responses.delete(args.rid as number);
            return null;
        default:
            return undefined;
    }
}
