/** 浏览器模式下“进程内的世界状态”：Anki 是否在运行等，随页面刷新复位 */
import { devSettings } from '../state';

/** 用户是否在本页面里通过应用启动过 Anki */
let ankiLaunched = false;

export function launchAnki(): void {
    ankiLaunched = true;
}

/** Anki 进程是否在运行：“Anki 未运行”状态下，启动后即视为运行且 AnkiConnect 可用 */
export function ankiRunning(): boolean {
    return devSettings.ankiState !== 'anki-not-running' || ankiLaunched;
}
