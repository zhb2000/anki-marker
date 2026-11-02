import type { DictSource } from './aiPick';

/**
 * 词典来源健康度的会话级统计（纯逻辑，不依赖 Vue）。
 *
 * 单次返空是常态——柯林斯没有短语、牛津没有生僻词——所以不能把“空结果”当故障。
 * 但接口变更导致字段静默失效时，表现就是“每次都返回空且不报错”，单次判定不出来。
 * 这里只做一件事：数出“某本词典连续多少次在别的词典有结果时返空”，把事实交给界面去说，
 * 不判断、不推断“是否故障”
 */

/** 一次词典查询的结果类型 */
export type DictQueryOutcome = 'filled' | 'empty' | 'failed' | 'skipped';

/** 连续返空达到该次数才提示：低于这个数更像“这几个词它本来就没有” */
export const EMPTY_STREAK_THRESHOLD = 5;

export interface DictHealth {
    /** 各词典连续返空的次数 */
    consecutiveEmpty: Record<DictSource, number>;
    /** 上一次计入连续空的查询词：同一个词反复查不累加 */
    countedWord: Record<DictSource, string>;
}

const SOURCES: readonly DictSource[] = ['collins', 'oxford', 'youdao'];

/** 新建一份空的健康度统计 */
export function createDictHealth(): DictHealth {
    return {
        consecutiveEmpty: { collins: 0, oxford: 0, youdao: 0 },
        countedWord: { collins: '', oxford: '', youdao: '' }
    };
}

/**
 * 记录一次三本词典的查询结果并更新连续返空次数。
 *
 * 只统计“至少另一本词典有结果”的查询：三本全空更可能是这个词不在任何词典里，
 * 不能算某本词典的问题。失败既不清零也不累加——那是另一种信号，由本次请求的失败列表单独提示。
 *
 * @param word 已归一化（trim + 小写）的查询词，用于同一词去重
 */
export function recordQuery(
    health: DictHealth,
    word: string,
    outcomes: Record<DictSource, DictQueryOutcome>
): void {
    const anyFilled = SOURCES.some(source => outcomes[source] === 'filled');
    for (const source of SOURCES) {
        const outcome = outcomes[source];
        if (outcome === 'filled') {
            health.consecutiveEmpty[source] = 0;
            health.countedWord[source] = '';
            continue;
        }
        if (outcome !== 'empty' || !anyFilled || health.countedWord[source] === word) {
            continue;
        }
        health.countedWord[source] = word;
        health.consecutiveEmpty[source] += 1;
    }
}

/** 该词典的连续返空是否已达到需要提示的程度 */
export function isEmptyStreakWorthNoticing(health: DictHealth, source: DictSource): boolean {
    return health.consecutiveEmpty[source] >= EMPTY_STREAK_THRESHOLD;
}
