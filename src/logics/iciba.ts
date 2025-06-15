import { MD5 } from './md5';


const ICIBA_TTS_C = 'word';
const ICIBA_TTS_M = 'translatetts';
const ICIBA_TTS_CLIENT = '6';
const ICIBA_TTS_SECRET_KEY = '#ICIBA!(*&R$@#LOVE#';

/**
 * 生成金山词霸 translateTTS API 请求所需的 sign。
 * 
 * sign = md5(c + m + secretKey + client + timestamp).hexdigest()[5:5 + 16]
 * 
 * @param timestamp - 当前时间戳，单位为秒。
 * @param options - 可选参数对象。
 * @returns 生成的 sign 字符串。
 */
function makeIcibaTtsSign(
    timestamp: number,
    {
        c = ICIBA_TTS_C,
        m = ICIBA_TTS_M,
        client = ICIBA_TTS_CLIENT,
        secretKey = ICIBA_TTS_SECRET_KEY
    } = {}
): string {
    const stringToSign = `${c}${m}${secretKey}${client}${timestamp}`;
    const md5Hash = new MD5(new TextEncoder().encode(stringToSign)).hexdigest();
    const sign = md5Hash.slice(5, 5 + 16);
    return sign;
}

/**
 * 生成金山词霸 translateTTS API 的请求 URL。
 * 
 * @param word - 要查询的文本。
 * @param options - 可选参数对象，包含以下属性：
 *   - tts_lan: 语音语言类型，默认为 '2'（EN_Female1）。
 *   - timestamp: 时间戳，默认为当前时间戳（单位为秒）。
 *   - c: 请求参数 c，默认为 'word'。
 *   - m: 请求参数 m，默认为 'translatetts'。
 *   - client: 客户端标识，默认为 '6'。
 *   - secretKey: 密钥，默认为 '#ICIBA!(*&R$@#LOVE#'。
 * @returns 完整的请求 URL 字符串。
 */
export function makeIcibaTtsUrl(
    word: string,
    {
        tts_lan = '2', // 默认使用 EN_Female1
        timestamp = null as number | null, // 如果为 null，则使用当前时间戳
        c = ICIBA_TTS_C,
        m = ICIBA_TTS_M,
        client = ICIBA_TTS_CLIENT,
        secretKey = ICIBA_TTS_SECRET_KEY
    } = {}
): string {
    // const timestamp = Math.floor(Date.now() / 1000);
    if (timestamp == null) {
        timestamp = Math.floor(Date.now() / 1000);
    }
    const sign = makeIcibaTtsSign(timestamp, { c, m, client, secretKey });
    const params = new URLSearchParams({
        c,
        m,
        client: client,
        timestamp: String(timestamp),
        sign,
        word,
        tts_lan
    });
    const baseUrl = 'https://dict-mobile.iciba.com/interface/index.php';
    return `${baseUrl}?${params.toString()}`;
}
