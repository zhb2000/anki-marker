import { fetch } from './net';
import { decodeHtmlEntities } from './stringutils';

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

/** 有道查询的超时（毫秒）：请求挂起时调用方（如 AI 优选等待预取结果）不能无限等下去 */
const YOUDAO_SEARCH_TIMEOUT_MS = 10000;

/**
 * 查询有道词典。HTTP 非 2xx、超时、返回内容不是合法 XML（如风控 HTML 页面）时抛出异常，
 * 不能当作“无结果”返回：调用方依赖异常来触发重试与错误提示。
 */
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
    // 超时同时覆盖等待响应头与读取响应体
    const controller = new AbortController();
    const timeoutId = window.setTimeout(() => controller.abort(), YOUDAO_SEARCH_TIMEOUT_MS);
    let xmlText: string;
    try {
        const response = await fetch(url, {
            method: 'GET',
            headers: {
                'Accept': 'text/xml',
                // 需要设置 Origin 请求头，
                // 或者设置为空字符串（Tauri 会移除请求头中的 Origin），
                // 否则有道词典 API 会返回 Invalid CORS request。
                'Origin': 'https://dict.youdao.com',
                'Connection': 'keep-alive'
            },
            signal: controller.signal
        });
        if (!response.ok) {
            response.body?.cancel().catch(() => { }); // 不读错误页内容，释放底层连接
            throw new Error(`有道词典查询失败：HTTP ${response.status}`);
        }
        xmlText = await response.text();
    } catch (error) {
        if (controller.signal.aborted) {
            throw new Error(`有道词典查询超时（${YOUDAO_SEARCH_TIMEOUT_MS / 1000} 秒）`, { cause: error });
        }
        throw error;
    } finally {
        clearTimeout(timeoutId);
    }
    const parser = new DOMParser();
    const xmlDoc = parser.parseFromString(xmlText, 'text/xml');
    // XML 解析失败时 DOMParser 不抛异常，而是返回含 <parsererror> 的文档（如服务端返回了 HTML 页面）
    if (xmlDoc.querySelector('parsererror') != null) {
        throw new Error('有道词典返回的内容不是合法的 XML');
    }
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