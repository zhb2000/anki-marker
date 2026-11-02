import { describe, expect, it } from 'vitest';

import type { CollinsItem, OxfordItem, YoudaoItem } from './dict';
import { LlmError } from './llm';
import {
    buildCandidates, buildResponseSchema, parseAiPickResult, shouldRetryWithNudge,
    type AiPick, type ParsedAiPick,
} from './aiPick';

const byId = new Map<string, AiPick>([
    ['co3f9', { source: 'collins', index: 0 }],
    ['ox7b2', { source: 'oxford', index: 3 }],
]);

function output(fields: Record<string, unknown>): string {
    return JSON.stringify({ kind: 'word', pick: null, contextual_def: '', note: '', ...fields });
}

function collins(sense: string, cnDef: string): CollinsItem {
    return { word: 'w', phonetic: null, sense, enDef: null, cnDef };
}

function oxford(sense: string, cnDef: string): OxfordItem {
    return { word: 'w', phrase: null, phonetic: null, sense, ext: null, enDef: null, cnDef };
}

function youdao(cnDef: string, meaningType: YoudaoItem['meaningType']): YoudaoItem {
    return { word: 'w', phonetic: null, sense: null, cnDef, meaningType };
}

function sources(collinsItems: CollinsItem[], oxfordItems: OxfordItem[], youdaoItems: YoudaoItem[]) {
    return { collins: collinsItems, oxford: oxfordItems, youdao: youdaoItems };
}

function parsed(overrides: Partial<ParsedAiPick>): ParsedAiPick {
    return {
        pick: null, contextualDef: '释义', note: '', fallback: true, kind: 'word', rawPick: null, ...overrides,
    };
}

describe('buildResponseSchema：结构化输出', () => {
    it('pick 的取值被限制在本次候选编号内，且允许 null', () => {
        const { name, schema } = buildResponseSchema(byId);
        expect(name).toBe('ai_pick');
        expect(schema['additionalProperties']).toBe(false);
        expect(schema['required']).toEqual(['kind', 'pick', 'contextual_def', 'note']);
        const properties = schema['properties'] as Record<string, Record<string, unknown>>;
        const pick = properties['pick'] as { anyOf: { enum?: string[]; type: string; }[]; };
        expect(pick.anyOf[0].enum).toEqual([...byId.keys()]);
        expect(pick.anyOf[1].type).toBe('null');
    });

    it('没有候选时 pick 只能是 null', () => {
        const { schema } = buildResponseSchema(new Map());
        const pick = (schema['properties'] as Record<string, Record<string, unknown>>)['pick'];
        expect(pick['type']).toBe('null');
    });
});

describe('shouldRetryWithNudge：什么时候值得重发', () => {
    it('模型明确不选（pick=null）时不重试：那是它的判断', () => {
        expect(shouldRetryWithNudge(parsed({ rawPick: null }), 12)).toBe(false);
    });

    it('给了编号但对不上时重试一次：那是输出错误', () => {
        expect(shouldRetryWithNudge(parsed({ rawPick: 'zz999' }), 12)).toBe(true);
    });

    it('没有候选时不存在“该选却没选”', () => {
        expect(shouldRetryWithNudge(parsed({ rawPick: 'zz999' }), 0)).toBe(false);
    });

    it('整句不重试：它本来就不该选候选', () => {
        expect(shouldRetryWithNudge(parsed({ rawPick: 'zz999', kind: 'sentence' }), 12)).toBe(false);
    });

    it('已经选中候选时不重试', () => {
        expect(shouldRetryWithNudge(
            parsed({ pick: { source: 'collins', index: 0 }, rawPick: 'co3f9', fallback: false }), 12,
        )).toBe(false);
    });
});

describe('buildCandidates：去重、排序与交错', () => {
    it('三本词典交错排列，不再按词典分块', () => {
        const built = buildCandidates(sources(
            [collins('a', '甲'), collins('b', '乙')],
            [oxford('c', '丙')],
            [youdao('丁', 'concise')],
        ));
        const order = built.lines.map(line => line.split(' ')[1]);
        expect(order).toEqual(['collins:', 'oxford:', 'youdao:', 'collins:']);
    });

    it('文本完全相同的义项只送一次，先出现的（柯林斯）留下', () => {
        const built = buildCandidates(sources(
            [collins('giving pleasure', '令人愉快的')],
            [oxford('giving pleasure', '令人愉快的')],
            [],
        ));
        expect(built.counts.collins).toBe(1);
        expect(built.counts.oxford).toBe(0);
        expect(built.lines).toHaveLength(1);
    });

    it('措辞不同但意思相近的义项都保留', () => {
        const built = buildCandidates(sources(
            [collins('giving pleasure', '令人愉快的')],
            [oxford('giving pleasure to the mind', '令人愉悦的')],
            [],
        ));
        expect(built.lines).toHaveLength(2);
    });

    it('有道优先送简明释义与短语，网络释义排最后', () => {
        const built = buildCandidates(sources(
            [],
            [],
            [youdao('网络一', 'web'), youdao('短语一', 'phrase'), youdao('简明一', 'concise')],
        ));
        const texts = built.lines.map(line => line.split('：')[1] ?? line);
        expect(texts[0]).toContain('简明一');
        expect(texts[1]).toContain('短语一');
        expect(texts[2]).toContain('网络一');
    });

    it('同一输入两次组装的编号一致，且能映射回界面索引', () => {
        const src = sources([collins('a', '甲'), collins('b', '乙')], [], []);
        const first = buildCandidates(src);
        const second = buildCandidates(src);
        expect([...first.byId.keys()]).toEqual([...second.byId.keys()]);
        for (const [id, pick] of first.byId) {
            expect(first.lines.some(line => line.startsWith(`[${id}]`))).toBe(true);
            expect(pick).toEqual({ source: 'collins', index: pick.index });
        }
    });

    it('没有释义的条目各自保留，不被当成重复项合并', () => {
        const built = buildCandidates(sources(
            [collins('', ''), collins('', '')],
            [],
            [],
        ));
        expect(built.lines).toHaveLength(2);
    });
});

describe('parseAiPickResult：按编号解析', () => {
    it('编号命中候选时取到该候选，并丢弃模型自带的释义', () => {
        const parsed = parseAiPickResult(output({ pick: 'ox7b2', contextual_def: '不该保留' }), byId);
        expect(parsed.pick).toEqual({ source: 'oxford', index: 3 });
        expect(parsed.contextualDef).toBe('');
        expect(parsed.fallback).toBe(false);
    });

    it('编号大小写与首尾空白宽容', () => {
        expect(parseAiPickResult(output({ pick: ' OX7B2 ' }), byId).pick).toEqual({ source: 'oxford', index: 3 });
    });

    it('编造的编号当作未选中：走 AI 生成', () => {
        const parsed = parseAiPickResult(output({ pick: 'zz999', contextual_def: '令人愉快的' }), byId);
        expect(parsed.pick).toBe(null);
        expect(parsed.fallback).toBe(true);
        expect(parsed.contextualDef).toBe('令人愉快的');
    });

    it('显式 null 且给了释义：AI 生成', () => {
        const parsed = parseAiPickResult(output({ pick: null, contextual_def: '令人愉快的' }), byId);
        expect(parsed.pick).toBe(null);
        expect(parsed.fallback).toBe(true);
    });

    it('整句：不选候选，保留译文', () => {
        const parsed = parseAiPickResult(output({ kind: 'sentence', contextual_def: '敏捷的棕色狐狸跳过懒狗。' }), byId);
        expect(parsed.kind).toBe('sentence');
        expect(parsed.pick).toBe(null);
        expect(parsed.contextualDef).toBe('敏捷的棕色狐狸跳过懒狗。');
    });

    it('拼写错误：既没有候选也没有释义，仍然算合法输出', () => {
        const parsed = parseAiPickResult(output({ kind: 'misspelling', note: '正确拼写应为 receive' }), byId);
        expect(parsed.kind).toBe('misspelling');
        expect(parsed.pick).toBe(null);
        expect(parsed.fallback).toBe(false);
        expect(parsed.note).toBe('正确拼写应为 receive');
    });

    it('缺省 kind 按单词处理', () => {
        const parsed = parseAiPickResult('{"pick":null,"contextual_def":"意思"}', byId);
        expect(parsed.kind).toBe('word');
    });

    it('既没有编号也没有释义、且不是拼错/乱码时视为坏响应', () => {
        expect(() => parseAiPickResult(output({ pick: null }), byId)).toThrow(LlmError);
    });

    it('保留模型原样给出的 pick，便于区分“明确没选”和“编号对不上”', () => {
        expect(parseAiPickResult(output({ pick: 'zz999', contextual_def: '释义' }), byId).rawPick).toBe('zz999');
        expect(parseAiPickResult(output({ pick: null, contextual_def: '释义' }), byId).rawPick).toBe(null);
    });

    it('输出不是 JSON 时报坏响应', () => {
        expect(() => parseAiPickResult('抱歉，我无法完成', byId)).toThrow(LlmError);
    });
});
