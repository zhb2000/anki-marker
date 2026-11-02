import type { DictSource } from './aiPick';

/**
 * 自动预取的熔断器（纯逻辑，不依赖 Vue）。
 *
 * 只用于 AI 之前的自动预取，不用于用户明确发起的查询：手打、回车、切词典 tab 都是
 * “我现在就要这个结果”，即使大概率失败也应该真的发出去，让用户看到真实错误，
 * 而不是由系统替他决定“别查了”。
 *
 * 有道是唯一走网络的词典（10 秒超时），故障时每个 AI 请求都要先白等一个超时。
 * 连续失败超过阈值后跳过一段时间，冷却结束放一次真实请求出去：成功即恢复，失败立即重新跳过。
 */

export interface PrefetchBreakerOptions {
    /** 连续失败达到该次数后开始跳过 */
    tripAfter: number;
    /** 跳过多长时间（毫秒），到期后放一次探测请求 */
    cooldownMs: number;
    /** 取当前时间，测试注入用 */
    now?: () => number;
}

export interface PrefetchBreaker {
    /** 该词典当前是否处于跳过期 */
    shouldSkip(source: DictSource): boolean;
    recordSuccess(source: DictSource): void;
    recordFailure(source: DictSource): void;
}

export function createPrefetchBreaker(options: PrefetchBreakerOptions): PrefetchBreaker {
    const now = options.now ?? (() => Date.now());
    const state: Record<DictSource, { failures: number; skipUntil: number; }> = {
        collins: { failures: 0, skipUntil: 0 },
        oxford: { failures: 0, skipUntil: 0 },
        youdao: { failures: 0, skipUntil: 0 }
    };
    const reset = (source: DictSource): void => {
        state[source].failures = 0;
        state[source].skipUntil = 0;
    };
    return {
        shouldSkip(source) {
            return now() < state[source].skipUntil;
        },
        recordSuccess(source) {
            reset(source);
        },
        recordFailure(source) {
            const item = state[source];
            item.failures += 1;
            // 冷却到期后的探测若再次失败，failures 已在阈值之上，会立刻再次跳过
            if (item.failures >= options.tripAfter) {
                item.skipUntil = now() + options.cooldownMs;
            }
        }
    };
}
