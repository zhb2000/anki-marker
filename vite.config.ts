import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import svgLoader from 'vite-svg-loader';

// https://vitejs.dev/config/

export default defineConfig(async () => ({
    plugins: [vue(), svgLoader()],
    // Vite 8 默认 target 已提升到 `baseline-widely-available`（Chrome 111 / Safari 16.4）。
    // Tauri 使用系统 WebView，显式降低 target 以继续支持较旧的 WebView（如 macOS 12.3+ 的 Safari 15.4）。
    build: {
        target: 'es2022',
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
        port: 1420,
        strictPort: true,
    },
    // 3. to make use of `TAURI_DEBUG` and other env variables
    // https://tauri.studio/v1/api/config#buildconfig.beforedevcommand
    envPrefix: ["VITE_", "TAURI_"],
}));
