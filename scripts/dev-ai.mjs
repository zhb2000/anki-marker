/**
 * 给 AI/脚本用的一键开发环境：以 dev-bridge feature 启动 Tauri dev，等桥接就绪后打印可用地址。
 *
 *   node scripts/dev-ai.mjs          启动并保持运行（建议放后台跑，日志在标准输出）
 *   node scripts/dev-ai.mjs status   打印 JSON：vite、桥接是否可用、是否属于本 worktree（退出码 0 = 本 worktree 就绪）
 *   node scripts/dev-ai.mjs stop     停止由本脚本启动的 dev 环境（含 Tauri 应用与 Vite）
 *
 * 端口与并行实例由环境变量决定（严格模式，冲突即失败）：
 *   DEV_PORT=1430                     指定 Vite 端口（缺省 1420）
 *   DEV_INSTANCE=foo DEV_PORT=1430    并行跑完整 Tauri（独立 identifier/配置/日志），见 docs/dev-features.md
 *
 * 就绪后用 agent-browser / playwright-cli 打开打印出的地址：没有 Tauri 的浏览器会自动连到真实 Rust（桥接）。
 * dev-bridge 只用于源码树里的 debug 构建。
 */
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { resolveDevEnv } from './dev-env.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const infoFile = path.join(root, 'dev-bridge.local.json');
const pidFile = path.join(root, 'dev-ai.local.pid');

let devEnv;
try {
    devEnv = resolveDevEnv(process.env);
} catch (error) {
    console.error(`[dev-ai] ${error instanceof Error ? error.message : String(error)}`);
    process.exit(2);
}
const URL_BASE = devEnv.url;

/** 查询该端口上的 dev 环境：是否有 Vite、桥接是否可用、它属于哪个 worktree */
async function status() {
    const result = { vite: false, bridge: false, own: false, url: URL_BASE, port: devEnv.port, instance: devEnv.instance ?? null, root: null };
    try {
        const response = await fetch(`${URL_BASE}/__dev/bridge/status`, { headers: { 'X-Dev-Bridge': '1' } });
        result.vite = response.ok;
        if (response.ok) {
            const data = await response.json();
            result.bridge = data.available === true;
            result.root = data.root ?? null;
            result.own = data.root === root;
        }
    } catch {
        // 保持 false
    }
    return result;
}

/** 能否连上 host:port（有进程在监听） */
function canConnect(port, host) {
    return new Promise(resolve => {
        const socket = net.connect({ port, host });
        socket.setTimeout(500, () => { socket.destroy(); resolve(false); });
        socket.once('connect', () => { socket.destroy(); resolve(true); });
        socket.once('error', () => resolve(false));
    });
}

/**
 * 端口是否空闲。只靠“尝试监听”不可靠：macOS 上绑定通配地址可以和只绑在 localhost（::1）的
 * Vite 共存而误判空闲，所以同时用“尝试连接”探测 localhost 的 IPv6/IPv4 两个地址。
 */
async function portFree(port) {
    if ((await Promise.all([canConnect(port, '::1'), canConnect(port, '127.0.0.1')])).some(Boolean)) {
        return false;
    }
    return new Promise(resolve => {
        const server = net.createServer();
        server.once('error', () => resolve(false));
        server.listen(port, () => server.close(() => resolve(true)));
    });
}

if (process.argv[2] === 'status') {
    const result = await status();
    console.log(JSON.stringify(result));
    process.exit(result.vite && result.bridge && result.own ? 0 : 1);
}

if (process.argv[2] === 'stop') {
    try {
        const pid = Number(fs.readFileSync(pidFile, 'utf8'));
        process.kill(pid, 'SIGTERM');
        console.log(`已向 dev:ai (pid ${pid}) 发送停止信号`);
    } catch {
        console.log('没有在运行的 dev:ai');
    }
    process.exit(0);
}

// 严格模式：端口被占用就失败，并说明占用者（能识别时）与解决办法，不自动顺延
if (!(await portFree(devEnv.port))) {
    const holder = await status();
    console.error(`[dev-ai] 端口 ${devEnv.port} 已被占用` + (holder.root != null
        ? `：占用者是 ${holder.root} 的 dev 环境${holder.own ? '（就是本 worktree，已经在运行）' : ''}`
        : '（不是本项目的 dev 环境）') + '。');
    console.error('[dev-ai] 要并行开发，请换端口并隔离实例：DEV_INSTANCE=<名字> DEV_PORT=<新端口> npm run dev:ai');
    process.exit(3);
}

// 清掉上一次运行遗留的连接信息，避免误判“已就绪”
fs.rmSync(infoFile, { force: true });
fs.writeFileSync(pidFile, String(process.pid));
process.on('exit', () => fs.rmSync(pidFile, { force: true }));

let ready = false;
const child = spawn(
    process.execPath,
    [path.join(root, 'scripts', 'tauri.mjs'), 'dev', '--features', 'dev-bridge'],
    { cwd: root, stdio: 'inherit', env: { ...process.env, VITE_DEV_FEATURES: 'true' } },
);
child.on('exit', code => {
    if (!ready) {
        console.error('[dev-ai] 开发环境在就绪之前就退出了。常见原因：编译失败（看上面的输出）；' +
            (devEnv.instance === undefined
                ? '已有另一个使用相同 identifier 的 dev 应用在运行（单实例会让新进程直接退出），并行请设置 DEV_INSTANCE 与 DEV_PORT。'
                : `同名实例 ${devEnv.instance} 已在运行。`));
    }
    process.exit(code ?? 1);
});
for (const signal of ['SIGINT', 'SIGTERM']) {
    process.on(signal, () => child.kill(signal));
}

// 首次编译 Rust 可能要几分钟；就绪后打印一行固定格式的提示，便于 AI 用 grep/等待文本判断
for (let i = 0; i < 1200; i++) {
    await new Promise(resolve => setTimeout(resolve, 1000));
    const result = await status();
    if (result.vite && result.bridge && result.own) {
        ready = true;
        console.log(`\n[dev-ai] READY ${URL_BASE}/ (backend: bridge${devEnv.instance ? `, instance: ${devEnv.instance}` : ''}). 在浏览器里调用 __dev.help() 查看调试接口。\n`);
        break;
    }
}
