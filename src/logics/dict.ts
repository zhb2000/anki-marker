import { invoke } from './utils';
import * as youdao from './youdao';
import * as iciba from './iciba';
export { makeYoudaoDictVoiceUrl } from './youdao';

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
async function getWordBase(word: string): Promise<string | null> {
    const base = await invoke<string | null>('get_word_base', { word });
    return base;
}

/**
 * 尝试将单词转换为原型形式
 * 
 * @returns 
 * - 若单词有原型形式，则返回一个数组，第一个元素为原单词，第二个元素为原型形式。
 * - 否则返回一个只包含原单词的数组。
 */
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
    const searchResult = await youdao.searchYoudaoDict(query);
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

/**
 * 检查单个音频 URL 是否可播放。
 * 
 * @param url 要检查的音频 URL。
 * @param timeoutMs 此检查的超时时间（毫秒）。
 * @returns Promise，解析为 true（可播放）或 false（不可播放/错误/超时）。
 */
async function checkUrlPlayability(url: string, timeoutMs: number): Promise<boolean> {
    return new Promise<boolean>((resolve) => {
        const tempAudio = document.createElement('audio');
        tempAudio.preload = 'metadata'; // 只需要元数据来判断是否能播放

        let timeoutId: number | undefined = undefined;

        const cleanupAndResolve = (result: boolean) => {
            clearTimeout(timeoutId);
            tempAudio.removeEventListener('canplay', canPlayListener);
            tempAudio.removeEventListener('error', errorListener);
            tempAudio.removeEventListener('abort', errorListener);
            tempAudio.src = ''; // 释放音频元素可能占用的资源
            resolve(result);
        };

        const canPlayListener = () => cleanupAndResolve(true);
        const errorListener = () => cleanupAndResolve(false); // 'error' 和 'abort' 都视为加载失败

        tempAudio.addEventListener('canplay', canPlayListener, { once: true });
        tempAudio.addEventListener('error', errorListener, { once: true });
        tempAudio.addEventListener('abort', errorListener, { once: true });

        tempAudio.src = url;
        tempAudio.load(); // 显式开始加载资源

        timeoutId = window.setTimeout(() => {
            // console.warn(`检查音频 URL 超时: ${url}`);
            cleanupAndResolve(false); // 超时视为不可播放
        }, timeoutMs);
    });
}

/**
 * 检查 URL 数组中的音频是否可以播放，返回第一个有效的音频 URL 的索引（按数组顺序优先）。
 * 
 * @param urls 要检查的音频 URL 字符串数组。
 * @param timeoutMs 每个 URL 检查的超时时间（毫秒）。
 * @returns Promise，解析为第一个有效 URL 的索引；如果所有 URL 都无效，则为 null。
 */
async function findFirstValidAudio(urls: string[], timeoutMs: number): Promise<number | null> {
    if (urls.length === 0) {
        return null;
    }
    return new Promise<number | null>((resolveOuter) => {
        /** 是否已得到结果 */
        let isResolved = false;
        const numUrls = urls.length;
        /** 存储每个 URL 的检查结果: 'incomplete', 'valid', 'invalid' */
        const results = new Array<'incomplete' | 'valid' | 'invalid'>(numUrls).fill('incomplete');
        /** 已完成检查的 URL 数量 */
        let completedCount = 0;

        // 此函数在每次检查完成后尝试确定最终结果
        const attemptResolve = () => {
            if (isResolved) {
                return; // 主 Promise 已被解析，无需再做操作
            }
            for (let i = 0; i < numUrls; i++) {
                if (results[i] === 'valid') {
                    isResolved = true;
                    resolveOuter(i); // 找到第一个有效的 URL (按优先级)
                    return;
                } else if (results[i] === 'incomplete') {
                    // 发现一个更高优先级的 URL 尚未完成检查，
                    // 因此无法确定当前是否应宣布较低优先级的 URL 为胜者，或者是否所有都将失败。
                    return;
                }
                // 如果 results[i] === 'invalid'，则继续检查下一个 URL
            }
            if (completedCount === numUrls) { // 所有 URL 都无效
                isResolved = true;
                resolveOuter(null); // 所有 URL 都已检查且均无效
            }
        };

        for (let index = 0; index < urls.length; index++) {
            void (async () => {
                const url = urls[index];
                let isPlayable: boolean;
                try {
                    isPlayable = await checkUrlPlayability(url, timeoutMs);
                } catch (error) {
                    // checkUrlPlayability 被设计为总是 resolve(boolean)，所以理论上不会到这里。
                    // 但作为防御性编程，我们将任何意外错误视为不可播放。
                    isPlayable = false;
                    console.error(`检查音频 URL '${url}' 时发生意外错误:`, error);
                }
                results[index] = isPlayable ? 'valid' : 'invalid';
                completedCount++; // 增加已完成检查的计数
                attemptResolve(); // 每次检查完成后，尝试解析主 Promise
            })();
        }
    });
}

export async function makePronunciationURL(word: string, pronunciationType: 'en' | 'us'): Promise<{ url: string, dict: 'youdao' | 'iciba'; } | null> {
    const youdaoPronunciationUrl = youdao.makeYoudaoPronounceUrl(word, { type: pronunciationType === 'en' ? '1' : '2' });
    const youdaoDictVoiceUrl = youdao.makeYoudaoDictVoiceUrl(word, pronunciationType);
    const icibaTtsUrl = iciba.makeIcibaTtsUrl(word);
    const urls = [youdaoPronunciationUrl, youdaoDictVoiceUrl, icibaTtsUrl];
    const dicts = ['youdao', 'youdao', 'iciba'] as const;
    const timeoutMs = 5000; // 设置超时时间为 5 秒
    const validIndex = await findFirstValidAudio(urls, timeoutMs);
    if (validIndex == null) {
        return null; // 所有音频 URL 都不可用
    }
    return {
        url: urls[validIndex],
        dict: dicts[validIndex]
    };
}

export function makePronunciationFilename(
    word: string,
    pronunciationType: 'en' | 'us',
    dict: 'youdao' | 'iciba'
): string {
    if (dict === 'iciba') {
        pronunciationType = 'en'; // iciba only supports English pronunciation
    }
    return `${dict}_${word}_${pronunciationType}.mp3`;
}
