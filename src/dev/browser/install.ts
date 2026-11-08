/**
 * 浏览器模式：在没有 Tauri 的普通浏览器里运行完整前端。
 *
 * 做法是在 Tauri 的 IPC 层打桩（官方 mockIPC），应用代码不需要任何分支。两档后端：
 * - 桥接（dev-bridge 在运行，默认）：invoke 与事件转发到真实的 Rust 进程（词典库、配置读取、
 *   文件监听、真实网络请求……）；有副作用的少数命令仍由夹具接管，见 LOCAL_ONLY；
 * - 夹具：按 commands 表返回静态数据，不需要编译 Rust，适合只看界面状态。
 *
 * 局限：Chromium 不等同于 macOS/Linux 的 WebKit；系统取词、托盘、全局快捷键测不了；
 * 原生窗口同时也在跑同一份前端，划词事件等会被两边各收到一次。
 */
import { emit } from '@tauri-apps/api/event';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';

import pkg from '../../../package.json';
import { CONFIG_DEFAULTS, type ConfigModel } from '../../logics/config';
import { devSettings } from '../state';
import { bridgeAvailable, bridgeInvoke, bridgeLogs, bridgeSubscribe, startBridgeEvents } from './bridge';
import { COLLINS_FIXTURES, OXFORD_FIXTURES, SAMPLE_CAPTURE } from './fixtures';
import { handleHttpCommand, PASSTHROUGH } from './http';
import { ankiRunning, launchAnki } from './runtime';

const CONFIG_STORAGE_KEY = 'anki-marker:dev-mock-config';
const warned = new Set<string>();

/** 浏览器里的“配置文件”（夹具模式的全部配置；桥接模式下是叠在真实配置之上的会话改动） */
function loadStoredConfig(): Partial<ConfigModel> {
    try {
        return JSON.parse(sessionStorage.getItem(CONFIG_STORAGE_KEY) ?? '{}') as Partial<ConfigModel>;
    } catch {
        return {};
    }
}

function saveStoredConfig(config: Partial<ConfigModel>): void {
    sessionStorage.setItem(CONFIG_STORAGE_KEY, JSON.stringify(config));
}

function detectPlatform(): 'macos' | 'windows' | 'linux' {
    if (devSettings.platform !== 'auto') {
        return devSettings.platform;
    }
    const ua = navigator.userAgent;
    return /Mac/i.test(ua) ? 'macos' : /Win/i.test(ua) ? 'windows' : 'linux';
}

function installOsPlugin(): void {
    const platform = detectPlatform();
    (window as unknown as Record<string, unknown>).__TAURI_OS_PLUGIN_INTERNALS__ = {
        os_type: platform,
        platform,
        family: platform === 'windows' ? 'windows' : 'unix',
        version: '0.0.0-dev-mock',
        arch: 'aarch64',
        eol: platform === 'windows' ? '\r\n' : '\n',
        exe_extension: platform === 'windows' ? 'exe' : '',
    };
}

let autostartEnabled = false;

/**
 * 桥接模式下始终由夹具处理的命令：它们会碰本机的真实环境（打开程序/目录、全局快捷键、
 * 开机自启动、剪贴板、原生窗口），AI 或脚本调试时不应有这些副作用。
 * 返回 PASSTHROUGH 表示不属于这一类，转发给 Rust。
 */
function localOnly(cmd: string, params: Record<string, any>): unknown {
    switch (cmd) {
        case 'show_in_explorer':
        case 'open_filepath':
        case 'open_in_browser':
            console.info(`[dev-mock] ${cmd}`, params);
            return null;
        case 'request_accessibility_trust':
        case 'suspend_global_shortcut':
        case 'resume_global_shortcut':
        case 'set_window_background':
            return null;
        case 'was_launched_hidden':
            return false;
        case 'plugin:autostart|is_enabled': return autostartEnabled;
        case 'plugin:autostart|enable': autostartEnabled = true; return null;
        case 'plugin:autostart|disable': autostartEnabled = false; return null;
        case 'plugin:clipboard-manager|read_text': return '';
        case 'plugin:window|theme': return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    }
    if (cmd.startsWith('plugin:window|') || cmd.startsWith('plugin:dialog|') || cmd.startsWith('plugin:clipboard-manager|')) {
        return null;
    }
    // Anki 的启动与探测：除非明确选了真实 Anki，否则由夹具按“Anki 状态”模拟
    if (devSettings.ankiState !== 'real') {
        if (cmd === 'is_anki_running') return ankiRunning();
        if (cmd === 'launch_anki') { launchAnki(); return null; }
    }
    return PASSTHROUGH;
}

/** 夹具模式：没有 Rust，所有命令都在这里应答 */
function fixtureCommand(cmd: string, params: Record<string, any>, config: Partial<ConfigModel>): unknown {
    switch (cmd) {
        case 'read_config': return { ...CONFIG_DEFAULTS, ...config };
        case 'commit_config': Object.assign(config, params.modified); saveStoredConfig(config); return null;
        case 'config_path': return '/dev-mock/config.toml';
        case 'is_portable': return false;
        case 'start_config_watcher': return true;
        case 'rust_in_release': return false;
        case 'is_anki_running': return ankiRunning();
        case 'launch_anki': launchAnki(); return null;
        case 'get_word_base': return null;
        case 'search_collins': return COLLINS_FIXTURES[String(params.word).toLowerCase()] ?? [];
        case 'search_oxford': return OXFORD_FIXTURES[String(params.word).toLowerCase()] ?? [];
        case 'get_shortcut_registration': return null;
        case 'is_accessibility_trusted': return true;
        case 'is_capture_settling': return false;
        case 'take_pending_sentence': return null;
        case 'take_pending_capture_failure': return null;
        case 'sanitize_filename': return String(params.filename).replace(/[\\/:*?"<>|]/g, '_');
        case 'plugin:app|version': return pkg.version;
        case 'plugin:log|log': return null;
        default:
            if (!warned.has(cmd) && !cmd.startsWith('plugin:event|')) {
                warned.add(cmd);
                console.warn(`[dev-mock] 未登记的命令 ${cmd}，已返回 null（需要时在 src/dev/browser/install.ts 补充）`, params);
            }
            return null;
    }
}

/** 安装浏览器模式，返回实际使用的后端 */
export async function installBrowserMock(): Promise<'bridge' | 'fixtures'> {
    const useBridge = devSettings.browserBackend === 'auto' && await bridgeAvailable();
    installOsPlugin();
    mockWindows('main');
    const stored = loadStoredConfig();

    mockIPC(async (cmd, args) => {
        const params = (args ?? {}) as Record<string, any>;
        if (!useBridge) {
            const http = cmd.startsWith('plugin:http|') ? await handleHttpCommand(cmd, params) : undefined;
            if (http !== undefined) {
                return http;
            }
            return localOnlyFixture(cmd, params, stored);
        }
        // 桥接：先看是否属于必须本地处理的命令，再看 http（AnkiConnect 仍走夹具），其余转发
        if (cmd.startsWith('plugin:http|')) {
            const http = await handleHttpCommand(cmd, params, true);
            if (http !== PASSTHROUGH) {
                return http;
            }
        } else {
            const local = localOnly(cmd, params);
            if (local !== PASSTHROUGH) {
                return local;
            }
        }
        if (cmd === 'commit_config' && devSettings.bridgeConfigWrites === 'session') {
            Object.assign(stored, params.modified);
            saveStoredConfig(stored);
            return null;
        }
        const result = await bridgeInvoke(cmd, params);
        if (cmd === 'read_config' && devSettings.bridgeConfigWrites === 'session') {
            return { ...(result as ConfigModel), ...stored };
        }
        return result;
    }, { shouldMockEvents: true });

    if (useBridge) {
        // 前端 listen 某个事件时，通知服务端开始转发该事件
        const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string, args?: Record<string, any>, options?: unknown) => Promise<unknown>; }; }).__TAURI_INTERNALS__;
        const inner = internals.invoke;
        internals.invoke = (cmd, args, options) => {
            if (cmd === 'plugin:event|listen' && typeof args?.event === 'string') {
                bridgeSubscribe(args.event);
            }
            return inner(cmd, args, options);
        };
        startBridgeEvents();
    }

    if (window.__dev != null) {
        window.__dev.emit = (event, payload) => emit(event, payload);
        window.__dev.sampleCapture = SAMPLE_CAPTURE;
        window.__dev.browserMode = true;
        window.__dev.backend = useBridge ? 'bridge' : 'fixtures';
        window.__dev.rustLogs = useBridge ? bridgeLogs : undefined;
    }
    return useBridge ? 'bridge' : 'fixtures';
}

/** 夹具模式的命令分发：先处理必须本地模拟的，再走夹具表 */
function localOnlyFixture(cmd: string, params: Record<string, any>, config: Partial<ConfigModel>): unknown {
    const local = localOnly(cmd, params);
    // localOnly 对 is_anki_running/launch_anki 在“真实 Anki”下会放行，夹具模式没有真实 Anki，统一模拟
    return local !== PASSTHROUGH ? local : fixtureCommand(cmd, params, config);
}
