import { describe, expect, it } from 'vitest';

import { effectiveLlmScenario } from './effective';
import { defaultDevSettings, type DevSettings } from './schema';
import { devMockStatus } from './status';

function settings(overrides: Partial<DevSettings> = {}): DevSettings {
    return { ...defaultDevSettings(), ...overrides };
}

describe('effectiveLlmScenario', () => {
    it('auto: Tauri 默认真实请求，浏览器默认模拟', () => {
        expect(effectiveLlmScenario(settings(), 'tauri')).toBe('real');
        expect(effectiveLlmScenario(settings(), 'browser-bridge')).toBe('pick');
        expect(effectiveLlmScenario(settings(), 'browser-fixtures')).toBe('pick');
    });

    it('auto + 免 Key 预览 AI: 所有环境强制模拟', () => {
        const withMockConfig = settings({ mockLlmConfig: true });
        expect(effectiveLlmScenario(withMockConfig, 'tauri')).toBe('pick');
        expect(effectiveLlmScenario(withMockConfig, 'browser-bridge')).toBe('pick');
    });

    it('显式选择覆盖环境默认与免 Key 开关', () => {
        expect(effectiveLlmScenario(settings({ llmScenario: 'real', mockLlmConfig: true }), 'browser-bridge'))
            .toBe('real');
        expect(effectiveLlmScenario(settings({ llmScenario: 'thinking' }), 'tauri'))
            .toBe('thinking');
    });
});

describe('devMockStatus', () => {
    it('Tauri 默认不启用任何模拟，浏览器专属项被禁用', () => {
        const status = devMockStatus(settings(), 'tauri');
        expect(status.effective.llmScenario).toBe('real');
        expect(status.mocks).toEqual([]);
        expect(status.settings.platform.enabled).toBe(false);
        expect(status.settings.browserBackend.enabled).toBe(false);
        expect(status.settings.ankiState.enabled).toBe(false);
        expect(status.settings.bridgeConfigWrites.enabled).toBe(false);
        expect(status.settings.llmStreamDelayMs.enabled).toBe(false);
        expect(status.settings.mockLlmConfig.enabled).toBe(true);
    });

    it('浏览器桥接默认列出环境与 AI 模拟', () => {
        const status = devMockStatus(settings(), 'browser-bridge');
        expect(status.effective.llmScenario).toBe('pick');
        const keys = status.mocks.map(mock => mock.key);
        expect(keys).toEqual(expect.arrayContaining(['browserBackend', 'bridgeConfigWrites', 'llmScenario']));
    });

    it('夹具模式下选真实 Anki / 真实更新 / 真实模板会给出警告', () => {
        const status = devMockStatus(settings({
            ankiState: 'real',
            appUpdateScenario: 'real',
            templateVersionScenario: 'real',
        }), 'browser-fixtures');
        const warningKeys = status.warnings.map(warning => warning.key);
        expect(warningKeys).toEqual(expect.arrayContaining([
            'ankiState', 'appUpdateScenario', 'templateVersionScenario',
        ]));
    });

    it('显式真实 LLM 时禁用免 Key 与流式延迟', () => {
        const status = devMockStatus(settings({ llmScenario: 'real', mockLlmConfig: true }), 'tauri');
        expect(status.effective.llmScenario).toBe('real');
        expect(status.settings.mockLlmConfig.enabled).toBe(false);
        expect(status.settings.llmStreamDelayMs.enabled).toBe(false);
        expect(status.mocks.map(mock => mock.key)).not.toContain('llmScenario');
    });

    it('Tauri 下开启免 Key 会强制模拟并列入清单', () => {
        const status = devMockStatus(settings({ mockLlmConfig: true }), 'tauri');
        expect(status.effective.llmScenario).toBe('pick');
        const keys = status.mocks.map(mock => mock.key);
        expect(keys).toEqual(expect.arrayContaining(['llmScenario', 'mockLlmConfig']));
    });
});
