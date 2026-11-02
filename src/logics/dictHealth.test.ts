import { describe, expect, it } from 'vitest';

import { EMPTY_STREAK_THRESHOLD, createDictHealth, isEmptyStreakWorthNoticing, recordQuery } from './dictHealth';

describe('dictHealth：连续返空统计', () => {
    it('返空且别的词典有结果时累加', () => {
        const health = createDictHealth();
        recordQuery(health, 'foo', { collins: 'filled', oxford: 'empty', youdao: 'filled' });
        recordQuery(health, 'bar', { collins: 'filled', oxford: 'empty', youdao: 'filled' });

        expect(health.consecutiveEmpty.oxford).toBe(2);
        expect(health.consecutiveEmpty.collins).toBe(0);
    });

    it('同一个词反复查不累加', () => {
        const health = createDictHealth();
        recordQuery(health, 'foo', { collins: 'filled', oxford: 'empty', youdao: 'filled' });
        recordQuery(health, 'foo', { collins: 'filled', oxford: 'empty', youdao: 'filled' });

        expect(health.consecutiveEmpty.oxford).toBe(1);
    });

    it('三本全空不计入：更可能是这个词本身不在任何词典里', () => {
        const health = createDictHealth();
        recordQuery(health, 'foo', { collins: 'empty', oxford: 'empty', youdao: 'empty' });

        expect(health.consecutiveEmpty).toEqual({ collins: 0, oxford: 0, youdao: 0 });
    });

    it('有结果即清零', () => {
        const health = createDictHealth();
        recordQuery(health, 'foo', { collins: 'filled', oxford: 'empty', youdao: 'filled' });
        recordQuery(health, 'bar', { collins: 'filled', oxford: 'empty', youdao: 'filled' });
        recordQuery(health, 'baz', { collins: 'filled', oxford: 'filled', youdao: 'filled' });

        expect(health.consecutiveEmpty.oxford).toBe(0);
    });

    it('查询失败既不清零也不累加', () => {
        const health = createDictHealth();
        recordQuery(health, 'foo', { collins: 'filled', oxford: 'empty', youdao: 'filled' });
        recordQuery(health, 'bar', { collins: 'filled', oxford: 'failed', youdao: 'filled' });

        expect(health.consecutiveEmpty.oxford).toBe(1);
    });

    it('达到阈值才值得提示', () => {
        const health = createDictHealth();
        for (let i = 0; i < EMPTY_STREAK_THRESHOLD - 1; i++) {
            recordQuery(health, `w${i}`, { collins: 'filled', oxford: 'empty', youdao: 'filled' });
        }
        expect(isEmptyStreakWorthNoticing(health, 'oxford')).toBe(false);

        recordQuery(health, 'last', { collins: 'filled', oxford: 'empty', youdao: 'filled' });
        expect(isEmptyStreakWorthNoticing(health, 'oxford')).toBe(true);
    });
});
