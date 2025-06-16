import eslint from '@eslint/js';
import tseslint from 'typescript-eslint';
import pluginVue from 'eslint-plugin-vue';
import vueParser from 'vue-eslint-parser';
import globals from 'globals';

export default tseslint.config(
    // 基础配置 & 插件推荐配置
    eslint.configs.recommended,
    ...tseslint.configs.recommendedTypeChecked,
    ...pluginVue.configs['flat/essential'],

    // 主要配置块，应用于所有相关文件
    {
        files: ['**/*.{js,ts,vue}'],
        languageOptions: {
            globals: globals.browser,
        },
        rules: {
            // 通用规则
            'prefer-const': 'warn',
            '@typescript-eslint/no-floating-promises': 'error',
            '@typescript-eslint/require-await': 'warn',
            '@typescript-eslint/no-explicit-any': 'off',
            '@typescript-eslint/no-unused-vars': 'warn',
        },
    },
    // 针对 .vue 文件的特定配置
    {
        files: ['**/*.vue'],
        languageOptions: {
            // 使用 vue-eslint-parser 作为主解析器
            parser: vueParser,
            parserOptions: {
                // 为 <script> 块指定 typescript-eslint 解析器
                parser: tseslint.parser,
                // 为类型检查规则提供 TypeScript 项目信息
                project: true,
                tsconfigRootDir: import.meta.dirname,
                // 告诉 ts-parser .vue 文件也需要被解析
                extraFileExtensions: ['.vue'],
            },
        },
        rules: {
            'vue/multi-word-component-names': 'warn'
        },
    },
    // 针对 .ts 文件的特定配置
    {
        files: ['**/*.ts'],
        languageOptions: {
            parser: tseslint.parser,
            parserOptions: {
                // 为类型检查规则提供 TypeScript 项目信息
                project: true,
                tsconfigRootDir: import.meta.dirname,
            }
        },
        rules: {},
    },
    // 忽略文件配置
    {
        ignores: [
            'dist',
            'node_modules',
            'eslint.config.js',
            '**/*.d.ts',
            '**/*.local',
            '**/*.local.*',
            'src-tauri',
            'vite.config.ts',
            'build-for-windows.js',
        ],
    },
);
