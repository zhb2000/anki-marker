import { AnkiConnectApi } from './anki-connect';
import { typeAssertion } from './typing';
import type { CollinsItem, OxfordItem, YoudaoItem } from './dict';
import { decodeHtmlEntities, escapeHTML } from './stringutils';

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

    /**
     * 添加一条划词助手单词笔记。
     *
     * audio 为 null 表示没有可用的发音地址，此时不附带音频：AnkiConnect 下载失败时不报错，
     * 而是把错误信息追加进“发音”字段，卡片上会出现一段错误文本。
     */
    public async addMarkerNote(
        deckName: string,
        modelName: string,
        fields: Fields,
        audio: { url: string; filename: string; } | null,
        signal?: AbortSignal
    ): Promise<number | null> {
        return await this.addNote(
            deckName,
            modelName,
            { ...fields }, // fields
            audio != null
                ? [{ url: audio.url, filename: audio.filename, fields: ['发音'] }]
                : undefined, // audio
            { allowDuplicate: true }, // options
            signal
        );
    }

    /**
     * 核对一条划词助手笔记是否其实已经写入 Anki，返回其笔记 id，没有则返回 null。
     *
     * 用于 addNote 超时或连接中断之后：前端放弃等待并不会让 Anki 停止写入，若直接按
     * “未添加”处理，用户重试就会产生重复笔记（allowDuplicate 为 true），且应用已失去对第一条的引用。
     * AnkiConnect 在 Anki 主线程上逐个处理请求，因此这里的查询会排在那次 addNote 之后执行，
     * 看到的是它完成后的结果。
     *
     * 判定条件：笔记 id（即创建时刻的毫秒时间戳）不早于 since，模板、牌组一致，
     * 且“单词”“例句”“释义”三个字段与写入的内容逐字一致。
     */
    public async findAddedMarkerNote(
        deckName: string,
        modelName: string,
        fields: Fields,
        since: number
    ): Promise<number | null> {
        // added:2 覆盖今天和昨天：写入恰好跨过 Anki 的每日分界点时也不会漏掉。
        // 不在查询串里写牌组、模板名：它们可能含 Anki 搜索语法的特殊字符，改为取回详情后比对
        const noteIds = await this.invoke<number[]>('findNotes', { query: 'added:2' });
        const candidates = noteIds.filter(id => id >= since);
        if (candidates.length === 0) {
            return null;
        }
        const notes = await this.invoke<{
            noteId: number;
            modelName: string;
            fields: Record<string, { value: string; order: number; }>;
            cards: number[];
        }[]>('notesInfo', { notes: candidates });
        const matched = notes.filter(note => note.modelName === modelName
            && isSameFieldValue(note.fields['单词']?.value, fields['单词'])
            && isSameFieldValue(note.fields['例句']?.value, fields['例句'])
            && isSameFieldValue(note.fields['释义']?.value, fields['释义']));
        for (const note of matched) {
            if (note.cards.length === 0) {
                continue;
            }
            const cards = await this.invoke<{ deckName: string; }[]>('cardsInfo', { cards: note.cards });
            if (cards.some(card => card.deckName === deckName)) {
                return note.noteId;
            }
        }
        return null;
    }
}

/**
 * 比较 Anki 中存储的字段值与写入值：Anki 默认会把字段文本规范化为 NFC，
 * 从 PDF 等来源取到的分解形式字符写入后会变样，比较前两边统一规范化
 */
function isSameFieldValue(stored: string | undefined, written: string | undefined): boolean {
    return (stored ?? '').normalize('NFC') === (written ?? '').normalize('NFC');
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

/**
 * 把纯文本转成可安全写入 Anki 字段的 HTML（Anki 字段按 HTML 渲染）。
 *
 * 先解码再转义：牛津词库里的部分词条本身已是实体编码（如 `A &amp; E`），直接转义会变成
 * `&amp;amp;`，卡片上显示出实体原文；先解码成纯文本再转义，对已编码与未编码的输入都只编码一次
 */
export function toFieldText(text: string): string {
    return escapeHTML(decodeHtmlEntities(text));
}

/**
 * 根据单词释义和例句生成划词助手单词笔记的字段。
 *
 * @param sentence 已是 HTML 的例句（调用方负责转义与加粗标记）
 * @param note AI 生成的笔记（纯文本），为空时不写“笔记”字段
 */
export function makeFields(
    dict: 'collins' | 'oxford' | 'youdao',
    item: CollinsItem | OxfordItem | YoudaoItem,
    sentence: string,
    note?: string | null
): Fields {
    let fields: Fields;
    if (dict === 'collins') {
        typeAssertion<CollinsItem>(item);
        fields = {
            '单词': toFieldText(item.word),
            '音标': item.phonetic != null ? toFieldText(item.phonetic) : undefined,
            '释义': makeMeaning(item) ?? undefined,
            '例句': sentence
        };
    } else if (dict === 'oxford') {
        typeAssertion<OxfordItem>(item);
        fields = {
            '单词': toFieldText(item.phrase ?? item.word),
            '音标': item.phonetic != null ? toFieldText(item.phonetic) : undefined,
            '释义': makeMeaning(item) ?? undefined,
            '例句': sentence
        };
    } else if (dict === 'youdao') {
        typeAssertion<YoudaoItem>(item);
        fields = {
            '单词': toFieldText(item.word),
            '音标': item.phonetic != null ? toFieldText(item.phonetic) : undefined,
            '释义': makeMeaningFromYoudao(item) ?? undefined,
            '例句': sentence
        };
    } else {
        throw new Error(`Unknown dict: ${String(dict)}`);
    }
    // LLM 输出是纯文本，可能含 sb. < sth.、A & B，也可能被网页里的句子诱导输出标签，
    // 必须转义后再写入（模板以 {{笔记}} 原样输出 HTML）
    if (note != null && note.length > 0) {
        fields['笔记'] = escapeHTML(note);
    }
    return fields;
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
