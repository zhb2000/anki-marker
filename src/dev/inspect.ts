/**
 * 调试观测层：让 AI（和人）不用在窗口里点点点、也不用人工复制日志，就能看到“刚才发生了什么”。
 *
 * 记录三类事实到一个环形缓冲区（页面刷新后清空）：
 * - console：log/info/warn/error/debug；
 * - error：未捕获异常、未处理的 Promise 拒绝；
 * - invoke：每次 Tauri 命令调用（命令、参数、成功/失败、耗时、结果摘要）。
 * 并通过 `window.__dev` 提供：mark / logs / trace / report / help（用法见 docs/dev-features.md）。
 * 典型闭环：`const m = __dev.mark(); /* 执行操作 *\/ await __dev.report(m)`。
 */

import { click, fill, idle, press, text, waitFor } from './act';
import { pendingMockActivity } from './activity';

type Level = 'debug' | 'info' | 'warn' | 'error';
type Kind = 'console' | 'error' | 'invoke';

export interface InspectEntry {
    id: number;
    /** 毫秒时间戳 */
    t: number;
    kind: Kind;
    level: Level;
    text: string;
    /** invoke 专用 */
    cmd?: string;
    ms?: number;
}

const CAPACITY = 1000;
const MAX_TEXT = 400;
/** 不单独记录的 invoke：自身会制造噪音或循环；plugin:http 由下面的 HTTP 专用逻辑合并成一条 */
const IGNORED_INVOKE = /^(plugin:log\||plugin:event\||plugin:http\|)/;

/** 进行中的 invoke 数量（idle() 据此判断“应用安静下来了”） */
let pendingInvokes = 0;
/** 最近一次 DOM 变化的时间（idle() 用） */
let lastMutation = Date.now();

const ring: InspectEntry[] = [];
let nextId = 1;

function push(entry: Omit<InspectEntry, 'id' | 't'>): void {
    ring.push({ id: nextId++, t: Date.now(), ...entry });
    if (ring.length > CAPACITY) {
        ring.shift();
    }
}

function clip(text: string): string {
    return text.length > MAX_TEXT ? `${text.slice(0, MAX_TEXT)}…(+${text.length - MAX_TEXT})` : text;
}

function show(value: unknown): string {
    if (typeof value === 'string') return value;
    if (value instanceof Error) return `${value.name}: ${value.message}`;
    try {
        return JSON.stringify(value) ?? String(value);
    } catch {
        return String(value);
    }
}

function installConsoleCapture(): void {
    const levels: Record<string, Level> = { log: 'info', info: 'info', debug: 'debug', warn: 'warn', error: 'error' };
    for (const [name, level] of Object.entries(levels)) {
        const original = console[name as 'log'].bind(console);
        console[name as 'log'] = (...args: unknown[]) => {
            push({ kind: 'console', level, text: clip(args.map(show).join(' ')) });
            original(...args);
        };
    }
    window.addEventListener('error', event => {
        push({ kind: 'error', level: 'error', text: clip(`${event.message} (${event.filename}:${event.lineno})`) });
    });
    window.addEventListener('unhandledrejection', event => {
        push({ kind: 'error', level: 'error', text: clip(`未处理的 Promise 拒绝: ${show(event.reason)}`) });
    });
}

/**
 * 包装 invoke 以记录调用轨迹；必须在 __TAURI_INTERNALS__.invoke 就绪后调用。
 *
 * 只在浏览器模式下可行：真实 Tauri 窗口里 invoke 是不可写、不可配置的属性，赋值会抛
 * TypeError，所以原生窗口里跳过（返回 false），观测层的 console / 报错 / DOM 静止判断照常可用。
 */
function installInvokeTrace(): boolean {
    const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>; }; }).__TAURI_INTERNALS__;
    const descriptor = internals == null ? undefined : Object.getOwnPropertyDescriptor(internals, 'invoke');
    if (internals == null || descriptor == null || !(descriptor.writable === true || descriptor.configurable === true)) {
        return false;
    }
    const inner = internals.invoke;
    /** rid → 这次 HTTP 请求的描述与开始时间；plugin:http 的一次请求由 fetch / fetch_send / read_body 多条 IPC 组成，合并成一条记录 */
    const httpRequests = new Map<number, { label: string; start: number; }>();
    internals.invoke = async (cmd, args, options) => {
        if (cmd.startsWith('plugin:http|')) {
            return traceHttp(inner, httpRequests, cmd, args as Record<string, any> | undefined, options);
        }
        if (IGNORED_INVOKE.test(cmd)) {
            return inner(cmd, args, options);
        }
        pendingInvokes++;
        const start = performance.now();
        const label = clip(`${cmd} ${show(args ?? {})}`);
        try {
            const result = await inner(cmd, args, options);
            push({ kind: 'invoke', level: 'debug', cmd, ms: Math.round(performance.now() - start), text: `ok ${label} -> ${clip(show(result))}` });
            return result;
        } catch (error) {
            push({ kind: 'invoke', level: 'error', cmd, ms: Math.round(performance.now() - start), text: `FAILED ${label} -> ${clip(show(error))}` });
            throw error;
        } finally {
            pendingInvokes--;
        }
    };
    return true;
}

/** HTTP 请求的可读描述：方法 + 主机/路径；JSON 体里有 action（AnkiConnect）时附上动作名 */
function describeHttp(config: { method?: string; url?: string; data?: number[] | null; }): string {
    let target = String(config.url ?? '?');
    try {
        const url = new URL(target);
        target = `${url.host}${url.pathname}`;
    } catch { /* 保持原样 */ }
    let action = '';
    if (config.data != null && config.data.length > 0) {
        try {
            const body = JSON.parse(new TextDecoder().decode(new Uint8Array(config.data))) as { action?: unknown; };
            if (typeof body.action === 'string') action = ` [${body.action}]`;
        } catch { /* 非 JSON 体 */ }
    }
    return `${config.method ?? 'GET'} ${target}${action}`;
}

/** 追踪一次 plugin:http 的 IPC：fetch 记下请求，fetch_send 完成时记一条“方法 目标 状态码 耗时”，其余 IPC 不单独记录 */
async function traceHttp(
    inner: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>,
    requests: Map<number, { label: string; start: number; }>,
    cmd: string,
    args: Record<string, any> | undefined,
    options: unknown,
): Promise<unknown> {
    if (cmd === 'plugin:http|fetch') {
        const label = describeHttp((args?.clientConfig ?? {}) as Record<string, any>);
        const rid = await inner(cmd, args, options) as number;
        requests.set(rid, { label, start: performance.now() });
        return rid;
    }
    if (cmd === 'plugin:http|fetch_send') {
        const request = requests.get(args?.rid as number);
        const label = request?.label ?? 'HTTP ?';
        pendingInvokes++;
        try {
            const result = await inner(cmd, args, options) as { status: number; };
            const ms = Math.round(performance.now() - (request?.start ?? performance.now()));
            const ok = result.status >= 200 && result.status < 400;
            push({ kind: 'invoke', level: ok ? 'debug' : 'warn', cmd: `http ${label}`, ms, text: `HTTP ${result.status} ${label}` });
            return result;
        } catch (error) {
            const ms = Math.round(performance.now() - (request?.start ?? performance.now()));
            push({ kind: 'invoke', level: 'error', cmd: `http ${label}`, ms, text: `FAILED ${label} -> ${clip(show(error))}` });
            throw error;
        } finally {
            pendingInvokes--;
        }
    }
    return inner(cmd, args, options);
}

/** 页面上可见的对话框与提示文本，AI 不用截图就能知道“弹了什么” */
function visibleMessages(): { dialogs: string[]; alerts: string[]; } {
    const texts = (selector: string) => [...document.querySelectorAll<HTMLElement>(selector)]
        .map(el => (el.innerText ?? '').trim().replace(/\s+/g, ' '))
        .filter(text => text.length > 0)
        .map(clip);
    return { dialogs: texts('[role="dialog"], [role="alertdialog"], dialog[open]'), alerts: texts('[role="alert"], [role="status"]') };
}

function logs(options: { since?: number; level?: Level; kind?: Kind; } = {}): InspectEntry[] {
    const order: Record<Level, number> = { debug: 0, info: 1, warn: 2, error: 3 };
    return ring.filter(entry =>
        entry.id > (options.since ?? 0)
        && (options.level === undefined || order[entry.level] >= order[options.level])
        && (options.kind === undefined || entry.kind === options.kind));
}

/**
 * 一次性回答“自从 since 以来发生了什么”：错误、警告、命令调用（失败的单列）、页面上的对话框与提示，
 * 以及 Rust 日志里最近的 WARN/ERROR（仅桥接可用）。
 */
async function report(since = 0) {
    const entries = logs({ since });
    const invokes = entries.filter(entry => entry.kind === 'invoke');
    let rustWarnings: string[] | undefined;
    const rustLogs = window.__dev?.rustLogs;
    if (rustLogs != null) {
        try {
            rustWarnings = (await rustLogs({ lines: 40, grep: '' })).split('\n')
                .filter(line => /\b(WARN|ERROR)\b/.test(line)).slice(-10);
        } catch {
            rustWarnings = undefined;
        }
    }
    return {
        cursor: nextId - 1,
        page: { url: location.pathname + location.search, visibility: document.visibilityState, title: document.title },
        errors: entries.filter(entry => entry.level === 'error' && entry.kind !== 'invoke').map(entry => entry.text),
        failedInvokes: invokes.filter(entry => entry.level === 'error').map(entry => entry.text),
        warnings: entries.filter(entry => entry.level === 'warn').map(entry => entry.text),
        invokes: invokes.map(entry => `${entry.cmd}${entry.cmd?.startsWith('http ') ? ` ${entry.text.split(' ')[0] === 'HTTP' ? entry.text.split(' ')[1] : 'FAILED'}` : ''} ${entry.ms}ms${entry.level === 'error' && !entry.cmd?.startsWith('http ') ? ' FAILED' : ''}`),
        ...visibleMessages(),
        mocks: window.__dev?.mockStatus?.() ?? null,
        rustRecentWarnings: rustWarnings,
    };
}

const HELP = `window.__dev —— AI 调试接口（详见 docs/dev-features.md）
  mark()                      返回当前游标；操作前调用，之后用它查看“这期间发生了什么”
  await report(since?)        自 since 以来的错误/警告/失败的命令/调用轨迹/页面对话框与提示/Rust 近期 WARN-ERROR
  logs({since,level,kind})    原始条目（kind: console|error|invoke）
  await rustLogs({lines,grep})  Rust 日志文件尾部（仅桥接；含前端 console 转发进来的内容）
  list() / get() / set(k,v) / reset(k?)   开发者设置（场景、平台等，仅当前页面有效）
  mockStatus()                当前环境下所有实际生效的模拟、无效值警告与禁用项；report() 也会带上它
  click(文字|选择器, nth?) / fill(placeholder|标签, 值, nth?) / press(键, 目标?)   按可见文字操作页面，无需元素编号；找不到或有多个同名元素时报错并列出候选（含所在区块文字），用 nth 指定第几个
  await idle({quietMs,timeoutMs})   等到没有进行中的命令/请求且 DOM 静止；用它代替 sleep（看不到定时器：有重试/宽限期的流程用 waitFor）
  await waitFor(文字|正则, {timeoutMs})  等页面出现某段文字，如错误对话框；超时返回 found:false
  text(max?)                  当前页面可见文字
  emit(event, payload)        模拟后端事件（浏览器模式）；sampleCapture 是一份示例划词载荷
  resetMock()                 清空 AI 结果缓存与参数兼容缓存，让下一次请求按当前场景重走
  backend / browserMode       当前后端：'bridge'（真实 Rust）| 'fixtures'（夹具）`;

/** 安装观测层。invoke 轨迹需在 IPC 就绪（原生已有 / 浏览器模式 mockIPC 之后）再装 */
export function installInspector(): void {
    installConsoleCapture();
    new MutationObserver(() => { lastMutation = Date.now(); })
        .observe(document.documentElement, { subtree: true, childList: true, attributes: true, characterData: true });
    if (window.__dev != null) {
        window.__dev.mark = () => nextId - 1;
        window.__dev.logs = logs;
        window.__dev.report = report;
        window.__dev.help = () => HELP;
        window.__dev.click = click;
        window.__dev.fill = fill;
        window.__dev.press = press;
        window.__dev.text = text;
        window.__dev.idle = idle;
        window.__dev.waitFor = waitFor;
    }
}

/** 记录一次由 dev 夹具直接响应的 HTTP 请求（不经过 Tauri IPC，需要单独补进调用轨迹） */
export function recordMockHttp(
    method: string,
    url: URL,
    status: number,
    ms: number,
    scenario: string,
    error?: unknown,
): void {
    const label = `${method} ${url.host}${url.pathname}`;
    const ok = status >= 200 && status < 400;
    push({
        kind: 'invoke',
        level: ok ? 'debug' : 'warn',
        cmd: `http ${label} [dev ${scenario}]`,
        ms: Math.round(ms),
        text: `HTTP ${status} ${label}${error !== undefined ? ` -> ${clip(show(error))}` : ''}`,
    });
}

/** 进行中的 invoke 数量（含 dev 夹具直接处理的 HTTP 请求） */
export function pendingInvokeCount(): number {
    return pendingInvokes + pendingMockActivity();
}

/** 最近一次 DOM 变化的时间戳 */
export function lastMutationAt(): number {
    return lastMutation;
}

export { installInvokeTrace };