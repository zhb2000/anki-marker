import { fetch } from '@tauri-apps/plugin-http';
import { decodeHtmlEntities } from './stringutils';

/** 表示一条网络释义的结构 */
interface WebTranslation {
    /** 网络释义的关键词。*/
    key: string;

    /** 关键词对应的多个释义值。*/
    values: string[];
}

/** 表示解析 XML 后的完整数据结构 */
interface SearchResult {
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
