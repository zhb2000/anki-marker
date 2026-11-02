import { describe, expect, it } from 'vitest';

import { createPrefetchBreaker } from './prefetchBreaker';

function setup(tripAfter = 2, cooldownMs = 60_000) {
    let now = 0;
    const breaker = createPrefetchBreaker({ tripAfter, cooldownMs, now: () => now });
    return { breaker, advance: (ms: number) => { now += ms; } };
}

describe('prefetchBreaker：连续失败后跳过', () => {
    it('未达阈值不跳过', () => {
        const { breaker } = setup();
        breaker.recordFailure('youdao');
        expect(breaker.shouldSkip('youdao')).toBe(false);
    });

    it('达到阈值后进入跳过期', () => {
        const { breaker } = setup();
        breaker.recordFailure('youdao');
        breaker.recordFailure('youdao');
        expect(breaker.shouldSkip('youdao')).toBe(true);
        expect(breaker.shouldSkip('collins')).toBe(false);
    });

    it('冷却到期后放行一次探测', () => {
        const { breaker, advance } = setup();
        breaker.recordFailure('youdao');
        breaker.recordFailure('youdao');
        advance(60_000);
        expect(breaker.shouldSkip('youdao')).toBe(false);
    });

    it('探测再失败立即重新跳过，不用重新累计', () => {
        const { breaker, advance } = setup();
        breaker.recordFailure('youdao');
        breaker.recordFailure('youdao');
        advance(60_000);
        expect(breaker.shouldSkip('youdao')).toBe(false);

        breaker.recordFailure('youdao');
        expect(breaker.shouldSkip('youdao')).toBe(true);
    });

    it('成功一次即清零', () => {
        const { breaker } = setup();
        breaker.recordFailure('youdao');
        breaker.recordSuccess('youdao');
        breaker.recordFailure('youdao');
        expect(breaker.shouldSkip('youdao')).toBe(false);
    });
});
