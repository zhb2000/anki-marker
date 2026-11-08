import { defineConfig, loadEnv } from "vite";
import vue from "@vitejs/plugin-vue";
import svgLoader from 'vite-svg-loader';
import { devSettingsPlugin } from './scripts/dev-settings-plugin.mjs';
import { devBridgeProxyPlugin } from './scripts/dev-bridge-proxy.mjs';
import { resolveDevEnvFromProcess } from './scripts/dev-env.mjs';

// https://vitejs.dev/config/

export default defineConfig(async ({ command, mode }) => {
    const root = '.'; // 相对 vite 进程的工作目录（项目根）
    // 端口由 DEV_PORT 决定（严格模式，见 scripts/dev-env.mjs）；夹具浏览器模式缺省 1421
    // 只在开发服务器里解析：构建（含 release）不受这些环境变量影响，也不会因它们不合法而失败
    const devEnv = command === 'serve' ? resolveDevEnvFromProcess({ browserMode: mode === 'browser' }) : null;
    // dev 功能总开关：仅 dev server 且 VITE_DEV_FEATURES=true 时开启；任何构建（含 release）恒为关闭，
    // 经 define 变成编译期常量，关闭时 src/dev 整个目录都不会进入产物
    const devFeatures = command === 'serve' && loadEnv(mode, root, 'VITE_').VITE_DEV_FEATURES === 'true';
    return {
        plugins: [vue(), svgLoader(), ...(devFeatures ? [devSettingsPlugin(root), devBridgeProxyPlugin(root)] : [])],
        define: {
            __DEV_FEATURES__: JSON.stringify(devFeatures),
        },
        // Vite 8 默认 target 已提升到 `baseline-widely-available`（Chrome 111 / Safari 16.4）。
        // Tauri 使用系统 WebView，显式降低 target 以继续支持较旧的 WebView（如 macOS 12.3+ 的 Safari 15.4）。
        build: {
            target: 'es2022',
            // 桌面应用从本地加载资源，刻意不做 code-splitting（全部打进单一 bundle），
            // 覆盖默认 500kB 警告阈值；仍保留 2MB 上限，bundle 异常膨胀时依然会告警
            chunkSizeWarningLimit: 2000,
        },
        css: {
            postcss: './postcss.config.cjs',
        },

        // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
        //
        // 1. prevent vite from obscuring rust errors
        clearScreen: false,
        // 2. tauri expects a fixed port, fail if that port is not available
        server: {
            port: devEnv?.port,
            strictPort: true,
            // Windows 下 cargo 编译会锁定 target/ 下的 exe，Vite 的 fs.watch
            // 访问被锁文件会抛 EBUSY 导致 dev server 崩溃（EBUSY: resource busy or locked），
            // 因此忽略 Rust 构建产物目录（Tauri 自身会监视 src-tauri 的源码变更）
            watch: {
                ignored: ["**/src-tauri/**"],
            },
        },
        // 3. to make use of `TAURI_DEBUG` and other env variables
        // https://tauri.studio/v1/api/config#buildconfig.beforedevcommand
        envPrefix: ["VITE_", "TAURI_"],
    };
});