import { invoke } from './utils';

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
