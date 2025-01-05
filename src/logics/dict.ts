import { invoke } from './utils';
import { searchYoudaoDict as searchYoudaoWebDict } from './youdao';

export interface CollinsItem {
    word: string;
    phonetic: string | null;
    sense: string | null;
    enDef: string | null;
    cnDef: string | null;
}

export interface OxfordItem {
    word: string;
    phrase: string | null;
    phonetic: string | null;
    sense: string | null;
    ext: string | null;
    enDef: string | null;
    cnDef: string | null;
}

/** 获取单词的原型 */
export async function getWordBase(word: string): Promise<string | null> {
    const base = await invoke<string | null>('get_word_base', { word });
    return base;
}

async function convertWord(word: string): Promise<[string] | [string, string]> {
    const base = await getWordBase(word);
    if (base != null) {
        return [word, base];
    }
    return [word];
}

export async function searchCollins(word: string, autoConvert: boolean = true): Promise<CollinsItem[]> {
    const words = autoConvert ? await convertWord(word) : [word];
    const promises = words.map(word => invoke<CollinsItem[]>('search_collins', { word }));
    const results: CollinsItem[][] = await Promise.all(promises);
    const items = results.flat();
    return items;
}

export async function searchOxford(word: string, autoConvert: boolean = true): Promise<OxfordItem[]> {
    const words = autoConvert ? await convertWord(word) : [word];
    const promises = words.map(word => invoke<OxfordItem[]>('search_oxford', { word }));
    const results: OxfordItem[][] = await Promise.all(promises);
    const items = results.flat();
    return items;
}

export interface YoudaoItem {
    word: string;
    phonetic: string | null;
    sense: string | null;
    cnDef: string | null;
    /**
     * 释义类型，可能的值有：
     * - 'concise'：简明释义
     * - 'web'：网络释义
     * - 'phrase'：短语释义
     */
    meaningType: 'concise' | 'web' | 'phrase';
}

/** 将有道词典的 custom-translation 的 content 为词性和释义 */
function splitYoudaoCustomTranslationContent(content: string): [string | null, string] {
    const match = content.match(/^([a-zA-Z.]+)\s+(.+)$/); // 匹配“词性”和“释义”部分
    if (match != null) {
        return [
            match[1], // 提取词性
            match[2].trim() // 提取释义并去除多余空格
        ];
    }
    // 没有词性，只有释义
    return [
        null,
        content.trim()
    ];
}

export async function searchYoudao(query: string): Promise<YoudaoItem[]> {
    const searchResult = await searchYoudaoWebDict(query);
    if (searchResult.customTranslations.length === 0) {
        return [];
    }
    let phonetic: string | null = null;
    if (searchResult.ukPhoneticSymbol != null) {
        phonetic = `英[${searchResult.ukPhoneticSymbol}]`;
    }
    if (searchResult.usPhoneticSymbol != null) {
        phonetic = (phonetic != null)
            ? `${phonetic} 美[${searchResult.usPhoneticSymbol}]`
            : `美[${searchResult.usPhoneticSymbol}]`;
    }
    if (searchResult.phoneticSymbol != null && phonetic == null) {
        phonetic = searchResult.phoneticSymbol;
    }
    const items: YoudaoItem[] = [];
    // 简明释义
    for (const customTranslation of searchResult.customTranslations) {
        const [sense, cnDef] = splitYoudaoCustomTranslationContent(customTranslation);
        items.push({
            word: searchResult.returnPhrase,
            phonetic,
            sense,
            cnDef,
            meaningType: 'concise'
        });
    }
    // 网络释义
    if (searchResult.webTranslationSame != null) {
        for (const value of searchResult.webTranslationSame.values) {
            items.push({
                word: searchResult.webTranslationSame.key,
                phonetic,
                sense: null,
                cnDef: value,
                meaningType: 'web'
            });
        }
    }
    // 短语释义
    for (const webTranslation of searchResult.webTranslations) {
        items.push({
            word: webTranslation.key,
            phonetic,
            sense: null,
            cnDef: webTranslation.values.join('；'),
            meaningType: 'phrase'
        });
    }
    return items;
}
