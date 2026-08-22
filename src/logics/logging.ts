// 将前端 console 消息与未捕获错误转发到 Rust 侧的统一日志（tauri-plugin-log）。
// 转发后的日志带 [webview] 标签，与 Rust 日志输出到同样的 target（终端/文件）。
import { debug, error, info, trace, warn } from '@tauri-apps/plugin-log';
import type { App, ComponentPublicInstance } from 'vue';

declare global {
    interface Window {
        __frontendLoggingInstalled?: boolean;
    }
}

function formatArg(arg: unknown): string {
    if (arg instanceof Error) {
        // WebKit 的 error.stack 不包含错误消息本身，需要手动拼上
        const head = `${arg.name}: ${arg.message}`;
        const stack = arg.stack ?? '';
        return stack.includes(arg.message) ? stack : `${head}\n${stack}`;
    }
    if (typeof arg === 'string') {
        return arg;
    }
    try {
        return JSON.stringify(arg) ?? String(arg);
    } catch {
        return String(arg);
    }
}

function formatArgs(args: unknown[]): string {
    return args.map(formatArg).join(' ');
}

type Logger = (message: string) => Promise<void>;

/** 相同消息去重窗口（毫秒）：窗口内的重复消息只计数不转发 */
const DEDUP_WINDOW_MS = 1000;

let lastMessage: string | null = null;
let lastLogger: Logger | null = null;
let lastSentAt = 0;
let suppressedCount = 0;
let flushTimer: ReturnType<typeof setTimeout> | undefined;

/** 冲刷去重计数：输出一条汇总日志（类似 devtools 控制台的重复计数角标） */
function flushSuppressed(): void {
    if (flushTimer != null) {
        clearTimeout(flushTimer);
        flushTimer = undefined;
    }
    if (suppressedCount > 0 && lastLogger != null && lastMessage != null) {
        const count = suppressedCount;
        suppressedCount = 0;
        lastLogger(`[repeated ${count + 1} times, duplicates suppressed] ${lastMessage}`).catch(() => {});
    }
}

/** 发送日志。与上一条相同且处于去重窗口内的消息只发第一条，其余计数抑制。 */
function sendLog(logger: Logger, message: string): void {
    const now = Date.now();
    if (message === lastMessage && now - lastSentAt < DEDUP_WINDOW_MS) {
        suppressedCount++;
        lastSentAt = now;
        if (flushTimer != null) {
            clearTimeout(flushTimer);
        }
        // 连续刷屏停止 1 秒后冲刷一次计数
        flushTimer = setTimeout(flushSuppressed, DEDUP_WINDOW_MS);
        return;
    }
    flushSuppressed();
    lastMessage = message;
    lastLogger = logger;
    lastSentAt = now;
    // 转发失败时静默忽略，避免递归报错
    logger(message).catch(() => {});
}

type ConsoleFnName = 'log' | 'debug' | 'info' | 'warn' | 'error';

function forwardConsole(fnName: ConsoleFnName, logger: Logger): void {
    const original = console[fnName];
    console[fnName] = (...args: unknown[]) => {
        original(...args);
        sendLog(logger, formatArgs(args));
    };
}

function componentName(instance: ComponentPublicInstance): string {
    const options = instance.$options as { name?: string; __name?: string };
    return options.name ?? options.__name ?? 'AnonymousComponent';
}

/** 从出错组件向上拼接组件链路，如 'App > HomeView > SearchBox' */
function formatComponentChain(instance: ComponentPublicInstance | null): string {
    const names: string[] = [];
    let current: ComponentPublicInstance | null = instance;
    while (current) {
        names.unshift(componentName(current));
        current = current.$parent;
    }
    return names.join(' > ');
}

/** 安装 console 转发与全局错误钩子。应在 app.mount 之前调用。 */
export function setupFrontendLogging(app: App): void {
    // 防止 vite HMR 重复执行 main.ts 导致重复包装 console
    if (window.__frontendLoggingInstalled) {
        return;
    }
    window.__frontendLoggingInstalled = true;

    forwardConsole('log', trace);
    forwardConsole('debug', debug);
    forwardConsole('info', info);
    forwardConsole('warn', warn);
    forwardConsole('error', error);

    window.addEventListener('error', (event) => {
        const where = `${event.filename}:${event.lineno}:${event.colno}`;
        const stack = event.error instanceof Error ? `\n${formatArg(event.error)}` : '';
        sendLog(error, `[onerror] ${event.message} at ${where}${stack}`);
    });

    window.addEventListener('unhandledrejection', (event) => {
        sendLog(error, `[unhandledrejection] ${formatArg(event.reason)}`);
    });

    app.config.errorHandler = (err, instance, info) => {
        sendLog(
            error,
            `[vue errorHandler] (${info}) ${formatArg(err)}\n[component] ${formatComponentChain(instance)}`,
        );
    };
}
