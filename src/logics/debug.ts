/**
 * 调试 mock 机制的业务侧入口。
 *
 * 全部 dev 功能由开发者设置驱动（见 src/dev/schema.ts 的注册表与 docs/dev-features.md）：
 * 设置页“开发者”入口、文件持久化、URL 参数与 `window.__dev` 都在 src/dev 下，
 * 这里只负责把当前设置翻译成业务代码关心的几个读取函数。
 *
 * 总开关 `VITE_DEV_FEATURES` 关闭（含所有 release 构建）时，每个函数都返回
 * “真实行为”，开发者设置不存在，也不会被读取。
 */
import * as semver from 'semver';

import * as api from '../tauri-api';
import type { EffectiveLlmScenario } from '../dev/effective';
import { devSettings } from '../dev/state';
import type { AppUpdateScenario, LlmScenario, TemplateVersionScenario } from '../dev/schema';
import type { LatestAppInfo } from './globals';

export type { AppUpdateScenario, LlmScenario, TemplateVersionScenario };

/** 应用更新检查的当前场景（'real' 表示真实请求） */
export function appUpdateScenario(): AppUpdateScenario {
    return __DEV_FEATURES__ ? devSettings.appUpdateScenario : 'real';
}

/** 笔记模板版本获取的当前场景（'real' 表示真实获取） */
export function templateVersionScenario(): TemplateVersionScenario {
    return __DEV_FEATURES__ ? devSettings.templateVersionScenario : 'real';
}

/** LLM 请求的当前实际场景（'real' 表示使用配置中的真实 API Key 发起请求；auto 已解析掉） */
export function llmScenario(): EffectiveLlmScenario {
    if (!__DEV_FEATURES__) {
        return 'real';
    }
    return window.__dev?.mockStatus?.().effective.llmScenario ?? 'real';
}

/**
 * 当前是否在用“模拟 AI 连接配置”预览 AI 功能。
 * 仅在 dev 构建、实际场景是模拟、且模拟配置开关生效时为 true。
 * release 构建恒为 false。
 */
export function mockLlmConfigActive(): boolean {
    if (!__DEV_FEATURES__) {
        return false;
    }
    return window.__dev?.mockStatus?.().mocks.some(mock => mock.key === 'mockLlmConfig') ?? false;
}

/**
 * dev 构建下是否解锁“登录时自动启动”设置的修改。
 *
 * dev 与生产安装读写同一份系统自启动注册（Linux 的 .desktop、Windows 的注册表 Run 值
 * 均按 productName 命名，macOS 登录项同名），dev 下修改会把 target/debug 二进制注册为
 * 自启动、覆盖生产注册，所以默认锁定。release 构建始终可修改（本函数只在 dev 构建里被问到）。
 */
export function launchAtLoginEditableInDev(): boolean {
    return __DEV_FEATURES__ && devSettings.launchAtLoginEditable;
}

/**
 * 应用启动时是否自动检查更新。
 *
 * - release：始终检查；
 * - dev：模拟场景不联网，启动时照常检查，以便调试红点等启动时 UI；
 *   真实场景下默认不检查，避免频繁重启耗尽 GitHub API 的每小时配额，
 *   需要时在开发者设置里打开“启动时真实检查更新”。
 */
export function startupAppUpdateCheckEnabled(): boolean {
    if (import.meta.env.PROD) {
        return true;
    }
    if (!__DEV_FEATURES__) {
        return false;
    }
    return devSettings.appUpdateScenario !== 'real' || devSettings.realUpdateCheckAtStartup;
}

/** 模拟网络请求延迟（毫秒数来自开发者设置） */
export function mockDelay(): Promise<void> {
    const ms = __DEV_FEATURES__ ? devSettings.mockDelayMs : 0;
    return new Promise(resolve => setTimeout(resolve, ms));
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
    '如需在开发时查看此内容，请在设置的“开发者”页把“应用更新检查”切到“有新版本”。'
].join('\n');

/** 获取当前应用的下一个（mock）次版本号 */
async function nextMockAppVersion(): Promise<string> {
    const current = await api.app.getVersion();
    return semver.inc(current, 'minor') ?? '999.999.999';
}

/**
 * 获取 mock 的最新应用版本信息（仅在 `appUpdateScenario() !== 'real'` 时调用）。
 *
 * - 'new-version'：基于当前应用版本动态生成下一个次版本号
 * - 'no-update'：与当前应用版本相同
 * - 'request-error'：抛出异常
 */
export async function mockLatestAppInfo(): Promise<LatestAppInfo> {
    await mockDelay();
    const scenario = appUpdateScenario();
    switch (scenario) {
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
            throw new Error(`mock request error (debug app update scenario: '${scenario}')`);
        default:
            throw new Error(`unexpected debug app update scenario: '${scenario}'`);
    }
}