/**
 * 调试 mock 机制。
 *
 * 开发时通过环境变量选择 mock 场景：在项目根目录的 `.env.development.local`
 * 文件中配置（该文件被 gitignore，不会误提交），修改后需重启 dev server：
 *
 * ```ini
 * VITE_APP_UPDATE_SCENARIO=new-version
 * VITE_TEMPLATE_VERSION_SCENARIO=low-version
 * ```
 *
 * 可用场景见 `AppUpdateScenario` 与 `TemplateVersionScenario`。
 * release 构建强制使用真实场景（'real'），mock 代码不会生效。
 */
import * as semver from 'semver';

import * as api from '../tauri-api';
import type { LatestAppInfo } from './globals';

/** 应用更新检查的调试场景 */
export type AppUpdateScenario =
    /** 正常请求 GitHub Release API */
    | 'real'
    /** 有新版本：红点、新版本链接与更新说明对话框 */
    | 'new-version'
    /** 无新版本：提示"当前应用已是最新版本" */
    | 'no-update'
    /** 请求失败：设置页弹出错误提示，启动时静默失败 */
    | 'request-error';

/** 笔记模板版本获取的调试场景 */
export type TemplateVersionScenario =
    /** 正常从 Anki 获取笔记模板版本 */
    | 'real'
    /** 模板版本过低：显示红点、新版本链接与"更新模板"按钮 */
    | 'low-version'
    /** 获取失败：显示"获取失败" */
    | 'request-error';

const APP_UPDATE_SCENARIOS = ['real', 'new-version', 'no-update', 'request-error'] as const;
const TEMPLATE_VERSION_SCENARIOS = ['real', 'low-version', 'request-error'] as const;

/** 读取环境变量中的场景值，非法或缺省时回退为 'real' */
function readScenario<T extends string>(value: unknown, allowed: readonly T[], fallback: T): T {
    return typeof value === 'string' && (allowed as readonly unknown[]).includes(value)
        ? value as T
        : fallback;
}

/** 应用更新检查的调试场景（release 构建强制为 'real'） */
export const appUpdateScenario: AppUpdateScenario = import.meta.env.PROD
    ? 'real'
    : readScenario(import.meta.env.VITE_APP_UPDATE_SCENARIO, APP_UPDATE_SCENARIOS, 'real');

/** 笔记模板版本获取的调试场景（release 构建强制为 'real'） */
export const templateVersionScenario: TemplateVersionScenario = import.meta.env.PROD
    ? 'real'
    : readScenario(import.meta.env.VITE_TEMPLATE_VERSION_SCENARIO, TEMPLATE_VERSION_SCENARIOS, 'real');

/**
 * 应用启动时是否自动检查更新。
 *
 * dev 模式下真实场景（'real'）禁用启动检查，避免频繁请求 GitHub API；
 * mock 场景不打真实 API，自动启用启动检查，以便调试红点等启动时 UI。
 */
export function startupAppUpdateCheckEnabled(): boolean {
    return appUpdateScenario !== 'real' || import.meta.env.PROD;
}

/** mock 场景统一的请求延迟（毫秒），用于模拟网络请求 */
const MOCK_DELAY_MS = 1000;

/** 模拟网络请求延迟 */
export function mockDelay(): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, MOCK_DELAY_MS));
}

/** mock 更新说明的内容（覆盖 markdown 渲染的各种元素，并足够长以测试对话框滚动） */
const MOCK_CHANGELOG = [
    '## 新增',
    '',
    '- 支持划词后自动查询多本词典',
    '  - 柯林斯词典',
    '  - 新牛津英汉双解大词典',
    '  - 有道在线词典',
    '- 支持自定义 Anki Connect 的服务地址',
    '- 新增设置页面，支持修改配置文件并实时生效',
    '',
    '## 修复',
    '',
    '1. 修复了部分情况下划词面板无法正确分词的问题',
    '2. 修复了暗色模式下更新说明对话框的样式问题',
    '3. 修复了便携模式下配置文件路径解析的错误',
    '',
    '## 其他',
    '',
    '**性能优化**：*启动速度*提升约 30%，查词响应更快。',
    '',
    '- 行内代码：`npm run tauri dev`',
    '- 链接：[GitHub Release 页面](https://github.com/zhb2000/anki-marker/releases)',
    '',
    '```bash',
    '# 代码块示例',
    'npm install',
    'npm run tauri dev',
    '```',
    '',
    '| 表头一 | 表头二 |',
    '| ------ | ------ |',
    '| 单元格 | 单元格 |',
    '',
    '> 引用块示例：这是一段引用文字，用于测试更新说明对话框的渲染效果。',
    '',
    '---',
    '',
    '这是一段用于测试更新说明对话框渲染效果的假更新日志（debug mock 场景），内容刻意写得足够长，以便测试对话框的滚动效果。'.repeat(3),
    '',
    '如需在开发时查看此内容，请在项目根目录的 `.env.development.local` 中配置 `VITE_APP_UPDATE_SCENARIO=new-version`，然后重启 dev server。'
].join('\n');

/** 获取当前应用的下一个（mock）次版本号 */
async function nextMockAppVersion(): Promise<string> {
    const current = await api.app.getVersion();
    return semver.inc(current, 'minor') ?? '999.999.999';
}

/**
 * 获取 mock 的最新应用版本信息（仅在 `appUpdateScenario !== 'real'` 时调用）。
 *
 * - 'new-version'：基于当前应用版本动态生成下一个次版本号
 * - 'no-update'：与当前应用版本相同
 * - 'request-error'：抛出异常
 */
export async function mockLatestAppInfo(): Promise<LatestAppInfo> {
    await mockDelay();
    switch (appUpdateScenario) {
        case 'new-version': {
            const version = await nextMockAppVersion();
            return {
                version,
                tagName: `v${version}`,
                htmlURL: `https://github.com/zhb2000/anki-marker/releases/tag/v${version}`,
                name: `Anki Marker v${version}`,
                body: MOCK_CHANGELOG
            };
        }
        case 'no-update': {
            const version = await api.app.getVersion();
            return {
                version,
                tagName: `v${version}`,
                htmlURL: `https://github.com/zhb2000/anki-marker/releases/tag/v${version}`,
                name: `Anki Marker v${version}`,
                body: ''
            };
        }
        case 'request-error':
            throw new Error(`mock request error (debug app update scenario: '${appUpdateScenario}')`);
        default:
            throw new Error(`unexpected debug app update scenario: '${appUpdateScenario}'`);
    }
}
