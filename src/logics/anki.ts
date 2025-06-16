import { AnkiConnectApi } from './anki-connect';
import { typeAssertion } from './typing';
import type { CollinsItem, OxfordItem, YoudaoItem } from './dict';
import { escapeHTML } from './stringutils';

/**
 * Anki 服务类，在 Anki Connect API 的基础上针对应用的需求进行了封装。
 */
export class AnkiService extends AnkiConnectApi {
    public constructor(url: string) {
        super(url);
    }

    /** 创建划词助手笔记模板 */
    public async createMarkerModel(modelName: string) {
        await this.createModel(
            modelName,
            ["单词", "音标", "释义", "笔记", "例句", "url", "发音"], // inOrderFields
            [
                {
                    "Name": "Card 1",
                    "Front": MODEL_FRONT,
                    "Back": MODEL_BACK
                }
            ], // cardTemplates
            MODEL_CSS
        );
    }

    /** 获取划词助手笔记模板的第一个 model template */
    private async getMarkerModelTemplate(modelName: string): Promise<{
        name: string;
        ord: number;
        /** 卡片正面模板 */
        qfmt: string;
        /** 卡片背面模板 */
        afmt: string;
    }> {
        const models = await this.findModelsByName([modelName]);
        if (models.length === 0) {
            throw new Error(`Model ${modelName} 不存在`);
        }
        const model = models[0];
        const templates = model.tmpls as {
            name: string;
            ord: number;
            qfmt: string;
            afmt: string;
        }[];
        if (templates.length === 0) {
            throw new Error(`Model ${modelName} 没有卡片模板`);
        }
        return templates[0];
    }

    /** 更新划词助手的笔记模板（更新 model templates 和 model styling） */
    public async updateMarkerModel(modelName: string) {
        const template = await this.getMarkerModelTemplate(modelName);
        const templateName = template.name;
        await this.updateModelTemplates(modelName, { [templateName]: { Front: MODEL_FRONT, Back: MODEL_BACK } });
        await this.updateModelStyling(modelName, MODEL_CSS);
    }

    /** 获取划词助手笔记模板的版本号 */
    public async getCardTemplateVersionByModelName(modelName: string): Promise<string | null> {
        const template = await this.getMarkerModelTemplate(modelName);
        return extractCardTemplateVersion(template.qfmt);
    }

    /** 添加一条划词助手单词笔记 */
    public async addMarkerNote(
        deckName: string,
        modelName: string,
        fields: Fields,
        audioURL: string,
        audioFilename: string
    ): Promise<number | null> {
        return await this.addNote(
            deckName,
            modelName,
            { ...fields }, // fields
            [
                {
                    url: audioURL,
                    filename: audioFilename,
                    fields: ['发音']
                }
            ], // audio
            { allowDuplicate: true } // options
        );
    }
}

/** 划词助手笔记模板的字段 */
interface Fields {
    '单词': string;
    '音标'?: string;
    '释义'?: string;
    '笔记'?: string;
    '例句': string;
    'url'?: string;
}

function makeMeaning(item: CollinsItem | OxfordItem): string | null {
    const meaning = [];
    let firstLine: string | null = `<i>${escapeHTML(item.sense ?? '')}</i> ${escapeHTML((item as { ext?: string; }).ext ?? '')}`.trim();
    if (firstLine === '<i></i>') {
        firstLine = null;
    } else {
        meaning.push(firstLine);
    }
    if (item.enDef != null) {
        if (firstLine != null) {
            meaning[0] = `${firstLine}<br>`;
        }
        // Collins 词典的 EnDef 含有 <b></b> 标签，不转义
        meaning.push(`<span class="endef">${item.enDef}</span>`);
    }
    if (item.cnDef != null) {
        if (item.enDef == null && firstLine != null) {
            meaning[0] = `${firstLine}<br>`;
        }
        meaning.push(`<span class="cndef">${escapeHTML(item.cnDef)}</span>`);
    }
    return (meaning.length > 0) ? meaning.join('<br>') : null;
}

function makeMeaningFromYoudao(item: YoudaoItem): string | null {
    const meaning = [];
    let firstLine: string | null = `<i>${escapeHTML(item.sense ?? '')}</i>`;
    if (firstLine === '<i></i>') {
        firstLine = null;
    } else {
        meaning.push(firstLine);
    }
    if (item.cnDef != null) {
        if (firstLine != null) {
            meaning[0] = `${firstLine}<br>`;
        }
        meaning.push(`<span class="cndef">${escapeHTML(item.cnDef)}</span>`);
    }
    return (meaning.length > 0) ? meaning.join('<br>') : null;
}

/** 根据单词释义和例句生成划词助手单词笔记的字段 */
export function makeFields(
    dict: 'collins' | 'oxford' | 'youdao',
    item: CollinsItem | OxfordItem | YoudaoItem,
    sentence: string
): Fields {
    if (dict === 'collins') {
        typeAssertion<CollinsItem>(item);
        return {
            '单词': item.word,
            '音标': item.phonetic ?? undefined,
            '释义': makeMeaning(item) ?? undefined,
            '例句': sentence
        };
    } else if (dict === 'oxford') {
        typeAssertion<OxfordItem>(item);
        return {
            '单词': item.phrase ?? item.word,
            '音标': item.phonetic ?? undefined,
            '释义': makeMeaning(item) ?? undefined,
            '例句': sentence
        };
    } else if (dict === 'youdao') {
        typeAssertion<YoudaoItem>(item);
        return {
            '单词': item.word,
            '音标': item.phonetic ?? undefined,
            '释义': makeMeaningFromYoudao(item) ?? undefined,
            '例句': sentence
        };
    } else {
        throw new Error(`Unknown dict: ${String(dict)}`);
    }
}

import MODEL_FRONT from '../assets/model-front.html?raw';
import MODEL_BACK from '../assets/model-back.html?raw';
import MODEL_CSS from '../assets/model-css.css?raw';

function extractCardTemplateInfo(html: string): Record<string, any> | null {
    const parser = new DOMParser();
    // 解析 HTML 字符串为一个 Document 对象
    const doc = parser.parseFromString(html, 'text/html');
    const scriptElement = doc.getElementById('com.zhb2000.anki-marker_card-template-info');
    if (scriptElement != null) {
        // 获取 script 节点的文本内容
        const jsonString = scriptElement.textContent;
        if (jsonString != null) {
            // 解析 JSON 字符串为对象并返回
            return JSON.parse(jsonString) as Record<string, any>;
        } else {
            return null;
        }
    }
    return null;
}

function extractCardTemplateVersion(html: string): string | null {
    const info = extractCardTemplateInfo(html);
    return (info != null && typeof info.version === 'string') ? info.version : null;
}

/** 应用内置的卡片模板的版本号 */
export const CARD_TEMPLATE_VERSION = extractCardTemplateVersion(MODEL_FRONT)!;
