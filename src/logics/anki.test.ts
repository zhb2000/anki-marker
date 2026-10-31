import { describe, expect, it } from 'vitest';

import { makeFields, toFieldText } from './anki';
import type { CollinsItem, OxfordItem, YoudaoItem } from './dict';
import { decodeHtmlEntities } from './stringutils';

const collins: CollinsItem = {
    word: 'R&D <lab>',
    phonetic: '/ɑːr ən ˈdiː/',
    sense: 'N-UNCOUNT',
    enDef: '<b>R&amp;D</b> is research and development.',
    cnDef: '研发 & 开发 <注>',
};

const oxford: OxfordItem = {
    word: 'take',
    phrase: 'take sb. <aback>',
    phonetic: null,
    sense: 'phrasal verb',
    ext: 'informal',
    enDef: 'to surprise',
    cnDef: '使吃惊',
};

const youdao: YoudaoItem = {
    word: 'A &amp; E',
    phonetic: null,
    sense: 'n.',
    cnDef: '急诊室 <科>',
    meaningType: 'concise',
};

describe('toFieldText', () => {
    it.each([
        ['A & B < C', 'A &amp; B &lt; C'],
        // 牛津词库里已是实体编码的词条只编码一次，不会变成 &amp;amp;
        ['A &amp; E', 'A &amp; E'],
        ['plain text', 'plain text'],
        ['', ''],
    ])('%j → %j', (input, expected) => {
        expect(toFieldText(input)).toBe(expected);
    });

    // 文本完整性：写入字段的 HTML 在 Anki 中渲染出的文本，必须与词条文本（解码后）逐字一致
    it.each([
        '<script>alert("x")</script>',
        'A &amp; E',
        "it's “curly” & 'straight'",
        'café — naïve',
        '多字节 文本',
    ])('渲染回文本与原词条一致：%j', input => {
        expect(decodeHtmlEntities(toFieldText(input))).toBe(decodeHtmlEntities(input));
    });
});

describe('makeFields', () => {
    const sentence = 'An <b>R&amp;D</b> lab &lt;here&gt;.';

    it('例句原样写入（调用方已转义），单词与音标经 toFieldText 转义', () => {
        const fields = makeFields('collins', collins, sentence);
        expect(fields['例句']).toBe(sentence);
        expect(fields['单词']).toBe('R&amp;D &lt;lab&gt;');
        expect(decodeHtmlEntities(fields['单词'])).toBe(collins.word);
        expect(fields['音标']).toBe(collins.phonetic);
        expect(fields['释义']).toBe('<i>N-UNCOUNT</i><br><br><span class="endef"><b>R&amp;D</b> is research and development.</span><br><span class="cndef">研发 &amp; 开发 &lt;注&gt;</span>');
        expect('笔记' in fields).toBe(false);
    });

    it('牛津词条优先写短语', () => {
        const fields = makeFields('oxford', oxford, sentence);
        expect(decodeHtmlEntities(fields['单词'])).toBe(oxford.phrase);
        expect(fields['音标']).toBeUndefined();
        expect(fields['释义']).toBe('<i>phrasal verb</i> informal<br><br><span class="endef">to surprise</span><br><span class="cndef">使吃惊</span>');
    });

    it('有道词条：已编码的单词只编码一次', () => {
        const fields = makeFields('youdao', youdao, sentence);
        expect(fields['单词']).toBe('A &amp; E');
        expect(fields['释义']).toBe('<i>n.</i><br><br><span class="cndef">急诊室 &lt;科&gt;</span>');
    });

    it('AI 笔记转义后写入，渲染回文本与原笔记一致', () => {
        const note = '常用搭配 sb. < sth.；A & B；<img src=x onerror=alert(1)>';
        const fields = makeFields('collins', collins, sentence, note);
        expect(fields['笔记']).not.toContain('<');
        expect(decodeHtmlEntities(fields['笔记']!)).toBe(note);
    });

    it.each([null, undefined, ''])('笔记为 %j 时不写“笔记”字段', note => {
        const fields = makeFields('youdao', youdao, sentence, note);
        expect('笔记' in fields).toBe(false);
    });
});
