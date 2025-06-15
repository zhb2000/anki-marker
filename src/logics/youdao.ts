import { fetch } from '@tauri-apps/plugin-http';
import { decodeHtmlEntities } from './stringutils';
import { MD5 } from './md5';

/** 表示一条网络释义的结构 */
export interface WebTranslation {
    /** 网络释义的关键词。*/
    key: string;

    /** 关键词对应的多个释义值。*/
    values: string[];
}

/** 表示解析 XML 后的完整数据结构 */
export interface SearchResult {
    /** 返回的查询短语，通常是用户查询的关键词。*/
    returnPhrase: string;

    /** 查询短语的音标，可能为空，例如 'mə'ʃi:n'。*/
    phoneticSymbol: string | null;

    ukPhoneticSymbol: string | null;

    usPhoneticSymbol: string | null;

    /** 翻译类型，例如 'ec' 表示英汉翻译。*/
    translationType: string | null;

    /** 自定义翻译结果的数组，例如英汉翻译，包含多个翻译内容。*/
    customTranslations: string[];

    webTranslationSame: WebTranslation | null;

    /** 网络释义结果的数组，包含关键词和对应的多个翻译。*/
    webTranslations: WebTranslation[];
}

// 参考：https://github.com/g8up/youDaoDict/issues/19
export async function searchYoudaoDict(query: string): Promise<SearchResult> {
    const params = new URLSearchParams({
        client: 'deskdict',
        keyfrom: 'chrome.extension',
        pos: '-1',
        doctype: 'xml',
        xmlVersion: '3.2',
        dogVersion: '1.0',
        vendor: 'unknown',
        appVer: '3.1.17.4208',
        ue: 'utf8',
        le: 'eng',
        q: query
    });
    const url = `https://dict.youdao.com/fsearch?${params.toString()}`;
    const response = await fetch(url, {
        method: 'GET',
        headers: {
            'Accept': 'text/xml',
            // 需要设置 Origin 请求头，
            // 或者设置为空字符串（Tauri 会移除请求头中的 Origin），
            // 否则有道词典 API 会返回 Invalid CORS request。
            'Origin': 'https://dict.youdao.com',
            'Connection': 'keep-alive'
        }
    });
    const xmlText = await response.text();
    const parser = new DOMParser();
    const xmlDoc = parser.parseFromString(xmlText, 'text/xml');
    // 提取 return-phrase 节点的内容
    const returnPhrase = decodeHtmlEntities(xmlDoc.querySelector('return-phrase')?.textContent ?? '');
    // 提取 phonetic-symbol 节点的内容
    const phoneticSymbol = xmlDoc.querySelector('phonetic-symbol')?.textContent ?? null;
    const ukPhoneticSymbol = xmlDoc.querySelector('uk-phonetic-symbol')?.textContent ?? null;
    const usPhoneticSymbol = xmlDoc.querySelector('us-phonetic-symbol')?.textContent ?? null;
    // 提取 custom-translation 数据
    const translationType = xmlDoc.querySelector('custom-translation type')?.textContent ?? null;
    const customTranslationNodes = xmlDoc.querySelectorAll('custom-translation translation');
    const customTranslations: string[] = Array
        .from(customTranslationNodes)
        .map(node => node.querySelector('content')?.textContent?.trim() ?? '');
    // 提取 web-translation 数据
    const webTranslationNodes = xmlDoc.querySelectorAll('yodao-web-dict web-translation');
    let webTranslationSame: WebTranslation | null = null;
    const webTranslations: WebTranslation[] = [];
    for (const node of webTranslationNodes) {
        const key = node.querySelector('key')?.textContent?.trim() ?? '';
        const valueNodes = node.querySelectorAll('trans value');
        const values = Array.from(valueNodes).map(valueNode => valueNode.textContent?.trim() ?? '');
        if (node.getAttribute('same') === 'true' || key === returnPhrase) {
            // <web-translation same="true"> 表示查询词的网络释义
            webTranslationSame = { key, values };
        } else {
            // 其余 <web-translation> 表示查询词的相关短语的网络释义
            webTranslations.push({ key, values });
        }
    }
    return {
        returnPhrase,
        phoneticSymbol,
        ukPhoneticSymbol,
        usPhoneticSymbol,
        translationType,
        customTranslations,
        webTranslationSame,
        webTranslations,
    };
}


const YOUDAO_PRONOUNCE_SECRET_KEY = 'U3uACNRWSDWdcsKm';

/**
 * Generates the sign signature for the Youdao Dictionary pronunciation API.
 *
 * @param params - A dictionary containing all request parameters.
 * @param secretKey - The signing secret key.
 * @returns The sign string.
 */
function makeYoudaoPronounceSign(params: Record<string, string>, secretKey: string): string {
    // 获取 pointParam 指定的参数顺序
    const paramOrder = params.pointParam?.split(',') ?? [];
    // 过滤掉 pointParam 中不存在的参数
    const signParams: Record<string, string> = {};
    for (const key of paramOrder) {
        if (key in params && key !== 'key') { // Exclude 'key' itself
            signParams[key] = params[key];
        }
    }
    // 构建查询字符串（按pointParam顺序）
    const queryParts: string[] = [];
    for (const key of paramOrder) {
        if (key in signParams && key !== 'key') { // Ensure key exists and is not 'key'
            const value = signParams[key];
            // Empty values also need to be included
            queryParts.push(`${key}=${value}`);
        }
    }
    // 添加签名密钥
    const baseString = queryParts.join('&');
    const signString = `${baseString}&key=${secretKey}`;
    // Calculate MD5
    return new MD5(new TextEncoder().encode(signString)).hexdigest();
}

/**
 * Generates the full URL for Youdao Dictionary word pronunciation.
 *
 * @param word - The word to query.
 * @param options - Optional parameters:
 *   - type: Pronunciation type, '1' for UK, '2' for US (default is '2').
 *   - mysticTime: A timestamp for the request, if null, uses current time.
 *   - secretKey: The secret key for signing, default is 'U3uACNRWSDWdcsKm'.
 * @returns The complete pronunciation URL.
 */
export function makeYoudaoPronounceUrl(
    word: string,
    {
        type = '2', // 1 for UK, 2 for US
        mysticTime = null as number | string | null,
        secretKey = YOUDAO_PRONOUNCE_SECRET_KEY
    } = {}
): string {
    if (mysticTime == null) {
        mysticTime = Date.now().toString();
    } else if (typeof mysticTime === 'number') {
        mysticTime = mysticTime.toString();
    }
    const baseParams = {
        product: 'webdict',
        appVersion: '1',
        client: 'web',
        mid: '1',
        vendor: 'web',
        screen: '1',
        model: '1',
        imei: '1',
        network: 'wifi',
        keyfrom: 'dick',
        keyid: 'voiceDictWeb',
        mysticTime,
        yduuid: 'abcdefg',
        le: '',
        phonetic: '',
        rate: '4',
        word: word,
        type,
        id: '',
        pointParam: 'appVersion,client,imei,keyfrom,keyid,mid,model,mysticTime,network,product,rate,screen,type,vendor,word,yduuid,key',
    };
    // Generate sign
    const sign = makeYoudaoPronounceSign(baseParams, secretKey);
    const queryParams = new URLSearchParams({ ...baseParams, sign });
    const queryString = queryParams.toString();
    return `https://dict.youdao.com/pronounce/base?${queryString}`;
}

export function makeYoudaoDictVoiceUrl(word: string, pronunciationType: 'en' | 'us'): string {
    const type = (pronunciationType === 'en') ? '1' : '2';
    const params = new URLSearchParams({ type, audio: word });
    const baseUrl = 'https://dict.youdao.com/dictvoice';
    return `${baseUrl}?${params.toString()}`;
}
