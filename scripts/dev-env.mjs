/**
 * 开发环境的端口与实例标识，由环境变量决定（vite.config.ts、scripts/tauri.mjs、scripts/dev-ai.mjs 共用，保证一致）。
 *
 * - DEV_PORT：Vite 开发服务器端口，严格模式——被占用就失败，绝不自动顺延。缺省 1420
 *   （夹具浏览器模式 `npm run dev:browser` 缺省 1421）。
 * - DEV_INSTANCE：并行实例名。设置后 Tauri 的 identifier、产品名带上该后缀，从而单实例互斥、
 *   配置目录、日志文件都与其他实例隔离，可与日常实例或其他 worktree 的实例同时运行。
 *   必须同时显式给出 DEV_PORT（避免默认撞上 1420）。不设则行为与以往完全一致。
 *
 * 多个 worktree 并行开发的用法见 docs/dev-features.md。
 */

export const DEFAULT_PORT = 1420;
export const BROWSER_MODE_DEFAULT_PORT = 1421;
/** 与 tauri.dev.conf.json 保持一致 */
const BASE_IDENTIFIER = 'com.zhb2000.anki-marker.dev';
const BASE_PRODUCT_NAME = 'Anki 划词助手 (Dev)';

/**
 * 解析并校验环境变量，非法时抛出带修复指引的 Error。
 * @param {NodeJS.ProcessEnv | Record<string, string | undefined>} env
 * @param {{ browserMode?: boolean }} [options]
 */
export function resolveDevEnv(env, options = {}) {
    const rawPort = env.DEV_PORT?.trim();
    const instance = env.DEV_INSTANCE?.trim() || undefined;
    let port = options.browserMode ? BROWSER_MODE_DEFAULT_PORT : DEFAULT_PORT;
    if (rawPort) {
        port = Number(rawPort);
        if (!Number.isInteger(port) || port < 1024 || port > 65535) {
            throw new Error(`DEV_PORT=${rawPort} 不合法：需要 1024–65535 之间的整数`);
        }
    }
    if (instance !== undefined) {
        if (!/^[a-z0-9][a-z0-9-]{0,23}$/.test(instance)) {
            throw new Error(`DEV_INSTANCE=${instance} 不合法：只能用小写字母、数字和连字符，以字母或数字开头，最长 24 位`);
        }
        if (!rawPort) {
            throw new Error(
                `设置了 DEV_INSTANCE=${instance} 时必须显式指定 DEV_PORT（例如 DEV_PORT=1430），` +
                `否则会默认占用 ${port}，与日常实例冲突`);
        }
    }
    return {
        port,
        instance,
        url: `http://localhost:${port}`,
        /** 实例隔离时的 Tauri 配置覆盖（identifier 与产品名），没有实例名时为空对象 */
        identity: instance === undefined ? {} : {
            identifier: `${BASE_IDENTIFIER}.${instance}`,
            productName: `${BASE_PRODUCT_NAME.slice(0, -1)} ${instance})`,
        },
    };
}

/** 从当前进程的环境变量解析（给没有 Node 类型的 vite.config.ts 用） */
export function resolveDevEnvFromProcess(options = {}) {
    return resolveDevEnv(process.env, options);
}

/**
 * 需要追加给 `tauri dev` 的配置覆盖（JSON 合并补丁）；与默认一致时返回 null。
 * 端口变化要同步 devUrl 与 devCsp 里的 HMR WebSocket 地址。
 */
export function tauriDevOverride(devEnv) {
    if (devEnv.port === DEFAULT_PORT && devEnv.instance === undefined) {
        return null;
    }
    return {
        ...devEnv.identity,
        build: { devUrl: devEnv.url },
        app: {
            security: {
                devCsp: {
                    'connect-src': `'self' ipc: http://ipc.localhost ws://localhost:${devEnv.port}`,
                },
            },
        },
    };
}
