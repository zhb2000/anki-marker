/**
 * 预览用的样本笔记：字段由真实的 makeFields 产出，保证 <span class="cndef">、
 * <b> 高亮、转义等与线上写进 Anki 的内容一致；发音与 url 在这套管线之外，手工补上。
 */
import { makeFields } from '../../logics/anki';

export interface CardSample {
    id: string;
    label: string;
    fields: Record<string, string>;
}

/** makeFields 对未取到的字段会留 undefined，这里去掉，等同 Anki 里字段为空 */
function defined(source: object): Record<string, string> {
    return Object.fromEntries(
        Object.entries(source).filter(([, value]) => typeof value === 'string')
    );
}

const SOUND = '[sound:anki-marker-dev.mp3]';

const collins = defined(makeFields(
    'collins',
    {
        word: 'serendipity',
        phonetic: '/ˌserənˈdɪpəti/',
        sense: 'N-UNCOUNT',
        enDef: '<b>Serendipity</b> is the luck of finding something good without looking for it.',
        cnDef: '意外发现珍奇事物的运气',
    },
    'It was pure <b>serendipity</b> that we met at the station.',
    '偶然相遇的运气；常用于描述“意外收获”'
));

const oxford = defined(makeFields(
    'oxford',
    {
        word: 'take',
        phrase: 'take sb. aback',
        phonetic: null,
        sense: 'phrasal verb',
        ext: 'informal',
        enDef: 'to surprise someone',
        cnDef: '使某人吃惊',
    },
    'The news <b>took</b> me <b>aback</b>.'
));

const long = defined(makeFields(
    'collins',
    {
        word: 'design',
        phonetic: '/dɪˈzaɪn/',
        sense: 'N-UNCOUNT',
        enDef: '<b>Design</b> is the art or process of deciding how something will look, '
            + 'work, or be made, especially when it is intended to be attractive as well as useful.',
        cnDef: '设计；构思；设计图样；为某物（尤其是既要好看又要好用的东西）决定其外观、'
            + '结构与制作方式的过程或艺术',
    },
    'The <b>design</b> of the new library, which was completed in 1997 after nearly a decade of '
        + 'planning and public debate, reflects the architect’s belief that a public building should '
        + 'feel less like an institution and more like a living room.',
    '作名词时指“设计”这一过程或结果；作动词时指“设计、构思”。'
        + '注意与 pattern（图案）、layout（版面布局）区分：design 更强调有意的规划。'
));

const empty = defined(makeFields(
    'youdao',
    {
        word: 'pneumonoultramicroscopicsilicovolcanoconiosis',
        phonetic: null,
        sense: 'n.',
        cnDef: '矽肺病（由吸入极细硅尘引起的肺部疾病）',
        meaningType: 'concise',
    },
    ''
));

export const SAMPLES: CardSample[] = [
    {
        id: 'full',
        label: '完整词条（音标 / 释义 / 例句 / 笔记 / url / 发音）',
        fields: { ...collins, '发音': SOUND, 'url': 'https://www.collinsdictionary.com/dictionary/english/serendipity' },
    },
    {
        id: 'minimal',
        label: '精简（无音标 / 无笔记 / 无 url / 无发音）',
        fields: oxford,
    },
    {
        id: 'long',
        label: '长例句 / 长释义 / 长笔记',
        fields: { ...long, '发音': SOUND, 'url': 'https://www.collinsdictionary.com/dictionary/english/design' },
    },
    {
        id: 'edge',
        label: '超长单词 / 无例句',
        fields: empty,
    },
];
