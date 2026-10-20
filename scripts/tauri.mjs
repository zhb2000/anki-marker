/**
 * Tauri CLI 包装器：`npm run tauri dev` 时自动追加开发专用配置
 * （`src-tauri/tauri.dev.conf.json`）。
 *
 * 为什么需要：单实例插件的互斥键（Windows 命名互斥量 / Linux D-Bus 名 / macOS
 * socket）以及 app 配置目录都由 identifier 派生。dev 与已安装版本共用 identifier 时，
 * 常驻中的正式版会被判定为首实例，dev 进程启动即退出并把焦点让给正式版，表现为
 * “dev 启动后什么都没发生”；即使没有单实例插件，两者还会共用 config.toml 互相覆盖。
 *
 * 只对 `dev` 子命令、且调用方未自带 `--config` 时追加；`build` / `info` 等
 * 子命令原样透传，正式包不受影响。
 */
import { spawn } from 'node:child_process';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const devConfigPath = path.join('src-tauri', 'tauri.dev.conf.json');
const cliEntry = path.join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');

/**
 * 计算传给 Tauri CLI 的参数：dev 子命令且未显式指定 --config 时追加开发配置，
 * 其余情况原样返回。
 */
export function resolveArgs(args) {
    const hasConfig = args.some((arg, index) => arg === '--config'
        || arg === '-c'
        || arg.startsWith('--config=')
        || args[index - 1] === '--config'
        || args[index - 1] === '-c');
    if (args[0] !== 'dev' || hasConfig) {
        return args;
    }
    // 紧跟在 dev 之后插入：放在末尾会落到 `--` 之后的“应用参数”里，CLI 便看不到它
    return [args[0], '--config', devConfigPath, ...args.slice(1)];
}

function main() {
    const child = spawn(process.execPath, [cliEntry, ...resolveArgs(process.argv.slice(2))], {
        cwd: root,
        stdio: 'inherit',
    });
    child.on('error', error => {
        console.error(error);
        process.exit(1);
    });
    child.on('exit', (code, signal) => {
        if (signal) {
            process.kill(process.pid, signal);
            return;
        }
        process.exit(code ?? 1);
    });
}

if (process.argv[1] !== undefined && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    main();
}
