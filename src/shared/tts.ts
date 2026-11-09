/**
 * 发音地址的纯函数实现：应用侧重发音探测与卡片模板侧的朗读共用同一份。
 *
 * 这段代码会同时打进应用和 Anki 卡片脚本，所以只能用标准 Web API，
 * 不得引用 Tauri、src/dev 或编译期常量（卡片构建期有白名单校验，见 scripts/card-template.mjs）。
 */
import { MD5 } from './md5';

const YOUDAO_PRONOUNCE_SECRET_KEY = 'U3uACNRWSDWdcsKm';

/**
 * 生成有道发音接口的签名。
 *
 * 参与签名的参数及顺序由 pointParam 给出，最后追加密钥再做 MD5。
 */
function makeYoudaoPronounceSign(params: Record<string, string>, secretKey: string): string {
    const paramOrder = params.pointParam?.split(',') ?? [];
    const signParams: Record<string, string> = {};
    for (const key of paramOrder) {
        if (key !== 'key' && key in params) {
            signParams[key] = params[key];
        }
    }
    const queryParts: string[] = [];
    for (const key of paramOrder) {
        if (key !== 'key' && key in signParams) {
            queryParts.push(key + '=' + signParams[key]);
        }
    }
    const signString = queryParts.join('&') + '&key=' + secretKey;
    return new MD5(new TextEncoder().encode(signString)).hexdigest();
}

/** 生成有道单词发音地址（type：1 英式，2 美式；mysticTime 为空时取当前时间） */
export function makeYoudaoPronounceUrl(
    word: string,
    {
        type = '2',
        mysticTime = null as number | string | null,
        secretKey = YOUDAO_PRONOUNCE_SECRET_KEY,
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
        word,
        type,
        id: '',
        pointParam: 'appVersion,client,imei,keyfrom,keyid,mid,model,mysticTime,network,product,rate,screen,type,vendor,word,yduuid,key',
    };
    const sign = makeYoudaoPronounceSign(baseParams, secretKey);
    const queryParams = new URLSearchParams({ ...baseParams, sign });
    return 'https://dict.youdao.com/pronounce/base?' + queryParams.toString();
}

/** 生成有道 dictvoice 地址（按英式/美式区分） */
export function makeYoudaoDictVoiceUrl(word: string, pronunciationType: 'en' | 'us'): string {
    const type = (pronunciationType === 'en') ? '1' : '2';
    const params = new URLSearchParams({ type, audio: word });
    return 'https://dict.youdao.com/dictvoice?' + params.toString();
}

const ICIBA_TTS_C = 'word';
const ICIBA_TTS_M = 'translatetts';
const ICIBA_TTS_CLIENT = '6';
const ICIBA_TTS_SECRET_KEY = '#ICIBA!(*&R$@#LOVE#';

/** 生成金山词霸 translateTTS 的签名：md5(c + m + secretKey + client + timestamp) 的第 6~21 位 */
function makeIcibaTtsSign(
    timestamp: number,
    {
        c = ICIBA_TTS_C,
        m = ICIBA_TTS_M,
        client = ICIBA_TTS_CLIENT,
        secretKey = ICIBA_TTS_SECRET_KEY,
    } = {}
): string {
    const stringToSign = c + m + secretKey + client + timestamp;
    const md5Hash = new MD5(new TextEncoder().encode(stringToSign)).hexdigest();
    return md5Hash.slice(5, 5 + 16);
}

/** 生成金山词霸 translateTTS 地址 */
export function makeIcibaTtsUrl(
    word: string,
    {
        tts_lan = '2',
        timestamp = null as number | null,
        c = ICIBA_TTS_C,
        m = ICIBA_TTS_M,
        client = ICIBA_TTS_CLIENT,
        secretKey = ICIBA_TTS_SECRET_KEY,
    } = {}
): string {
    if (timestamp == null) {
        timestamp = Math.floor(Date.now() / 1000);
    }
    const sign = makeIcibaTtsSign(timestamp, { c, m, client, secretKey });
    const params = new URLSearchParams({
        c,
        m,
        client,
        timestamp: String(timestamp),
        sign,
        word,
        tts_lan,
    });
    return 'https://dict-mobile.iciba.com/interface/index.php?' + params.toString();
}
