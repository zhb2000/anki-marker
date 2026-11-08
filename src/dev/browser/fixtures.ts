/** 浏览器模式的静态夹具：词典条目与有道 XML。数据刻意很少，够把各种卡片状态跑出来即可 */
import type { CollinsItem, OxfordItem } from '../../logics/dict';

export const COLLINS_FIXTURES: Record<string, CollinsItem[]> = {
    design: [
        {
            word: 'design', phonetic: 'dɪˈzaɪn', sense: 'VERB',
            enDef: 'If you design something, you plan what it should be like and how it should work.',
            cnDef: '设计；构思',
        },
        {
            word: 'design', phonetic: 'dɪˈzaɪn', sense: 'N-COUNT',
            enDef: 'A design is a drawing or plan from which something may be made.',
            cnDef: '设计图；图样',
        },
    ],
    run: [
        {
            word: 'run', phonetic: 'rʌn', sense: 'VERB',
            enDef: 'When you run, you move more quickly than when you walk.',
            cnDef: '跑；奔跑',
        },
    ],
};

export const OXFORD_FIXTURES: Record<string, OxfordItem[]> = {
    design: [
        {
            word: 'design', phrase: null, phonetic: 'dɪˈzaɪn', sense: 'noun',
            ext: null, enDef: 'a plan or drawing produced to show the look and function of something before it is made',
            cnDef: '设计；设计图',
        },
    ],
};

/** 有道词典的词条数据：custom-translation 为简明释义，web 为网络释义 */
export const YOUDAO_FIXTURES: Record<string, { phonetic: string; translations: string[]; web: { key: string; values: string[]; }[]; }> = {
    design: {
        phonetic: 'dɪ\'zaɪn',
        translations: ['n. 设计；图案；构思', 'vt. 设计；构思；计划'],
        web: [
            { key: 'design', values: ['设计', '图案'] },
            { key: 'graphic design', values: ['平面设计', '图形设计'] },
        ],
    },
    run: {
        phonetic: 'rʌn',
        translations: ['vi. 跑；运行；经营', 'n. 跑步；运转'],
        web: [{ key: 'run', values: ['运行', '奔跑'] }],
    },
};

/** 仿有道 fsearch 接口的 XML；词不在夹具里时只返回 return-phrase（与真实的“查无此词”一致） */
export function youdaoXml(word: string): string {
    const data = YOUDAO_FIXTURES[word.toLowerCase()];
    const phrase = `<return-phrase><![CDATA[${word}]]></return-phrase>`;
    if (data == null) {
        return `<?xml version="1.0" encoding="utf-8"?><yodaodict>${phrase}</yodaodict>`;
    }
    const translations = data.translations
        .map(text => `<translation><content><![CDATA[${text}]]></content></translation>`)
        .join('');
    const web = data.web
        .map(item => `<web-translation><key><![CDATA[${item.key}]]></key><trans>${item.values
            .map(value => `<value><![CDATA[${value}]]></value>`).join('')}</trans></web-translation>`)
        .join('');
    return '<?xml version="1.0" encoding="utf-8"?><yodaodict>' + phrase
        + `<phonetic-symbol><![CDATA[${data.phonetic}]]></phonetic-symbol>`
        + `<custom-translation><type>concise</type>${translations}</custom-translation>`
        + `<yodao-web-dict>${web}</yodao-web-dict></yodaodict>`;
}

/** 示例句子，供 `window.__dev.emit('sentence-captured', ...)` 一类的调用复用 */
export const SAMPLE_CAPTURE = {
    text: 'We need to design a system that can handle millions of requests per second.',
    word: 'design',
    wordOffset: 11,
    retry: false,
    context: { before: [], after: ['Reliability matters more than raw speed.'] },
};
