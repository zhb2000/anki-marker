import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';

// 前端单元测试配置：与 vite.config.ts 分开，避免把 dev server / 打包选项带进测试。
// 被测模块在顶层创建 DOM 元素（stringutils.ts 的转义缓冲、anki.ts 解析模板版本），因此用 happy-dom 环境
export default defineConfig({
    plugins: [vue()],
    test: {
        environment: 'jsdom',
        include: ['src/**/*.test.ts'],
    },
});
