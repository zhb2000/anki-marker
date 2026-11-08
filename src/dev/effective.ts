/**
 * “跟随环境”类设置的解析规则。
 *
 * 当前只有 LLM 场景需要按环境取默认值。不拆成两个设置项：同一个 LLM 场景在
 * Tauri 窗口与浏览器页应分别解析成不同的实际值，拆键会让两个值各自漂移，
 * 也会让 URL 参数/文件配置的优先级变得难解释。
 */
import type { DevEnvironment } from './environment';
import type { DevSettings, LlmScenario } from './schema';

/** 实际会执行的 LLM 场景：auto 已经被解析掉 */
export type EffectiveLlmScenario = Exclude<LlmScenario, 'auto'>;

/**
 * 解析当前实际生效的 LLM 场景。
 *
 * 优先级：
 * 1. 显式选择（real / 任一模拟场景）——用户明确指定，直接生效；
 * 2. 开启“免 Key 预览 AI”且场景为 auto——强制模拟（否则假配置没有用武之地）；
 * 3. auto：Tauri 默认真实请求，浏览器桥接/夹具默认模拟。
 */
export function effectiveLlmScenario(
    settings: Pick<DevSettings, 'llmScenario' | 'mockLlmConfig'>,
    environment: DevEnvironment,
): EffectiveLlmScenario {
    if (settings.llmScenario !== 'auto') {
        return settings.llmScenario;
    }
    if (settings.mockLlmConfig) {
        return 'pick';
    }
    return environment === 'tauri' ? 'real' : 'pick';
}
