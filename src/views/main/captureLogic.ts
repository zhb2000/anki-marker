import { isWord } from '../../logics/stringutils';
import type { SentenceToken } from './types';

/**
 * 划词捕获结果：text 为录入文本（取句成功时是整句），word 为取句模式命中的单词
 * （用于预选，可能为 null），wordOffset 为该词在 text 中的 UTF-16 起始偏移（不可用时为
 * null，预选回退到按字符串匹配），retry 标记迟到的后台补取结果。
 */
export interface CapturedSentencePayload {
    text: string;
    word: string | null;
    wordOffset: number | null;
    retry: boolean;
}

/**
 * 划词没有录入内容的原因（与后端 CaptureFailure 对应）：
 * empty = 目标应用里没有可取用的选区；failed = 取词过程报错
 */
export type CaptureFailure = 'empty' | 'failed';

/** 一次捕获载荷的处理结论 */
export type CapturedSentencePlan =
    /** 文本裁剪后为空：什么都不做 */
    | { kind: 'empty'; }
    /** 迟到的补取结果，而用户已经改动过工作区：丢弃 */
    | { kind: 'stale-retry'; }
    /** 录入：text 为裁剪后的句子；word 为裁剪后的命中词（缺失或为空时 null）；wordOffset 已按裁剪校验 */
    | { kind: 'apply'; text: string; word: string | null; wordOffset: number | null; };

/**
 * 决定如何处理一次捕获载荷。
 *
 * 迟到的补取结果（同一句话的后台重试）只在用户未改动时生效：用户已经编辑、输入或
 * 动手标记，说明工作区已被接管，补取不得覆盖
 */
export function planCapturedSentence(payload: CapturedSentencePayload, userTouched: boolean): CapturedSentencePlan {
    const trimmed = payload.text.trim();
    if (trimmed.length === 0) {
        return { kind: 'empty' };
    }
    if (payload.retry && userTouched) {
        return { kind: 'stale-retry' };
    }
    const word = payload.word?.trim();
    return {
        kind: 'apply',
        text: trimmed,
        word: (word != null && word.length > 0) ? word : null,
        // 偏移以载荷原文为基准：文本被 trim 改短时偏移不再可信
        wordOffset: trimmed.length === payload.text.length ? payload.wordOffset : null,
    };
}

export function isSameCapturedPayload(a: CapturedSentencePayload, b: CapturedSentencePayload): boolean {
    return a.text === b.text && a.word === b.word && a.wordOffset === b.wordOffset && a.retry === b.retry;
}

/**
 * 同一种捕获结果的两条送达路径：事件，以及主窗口重建期间后端暂存、前端就绪后取回的那份。
 *
 * 后端每次捕获都会暂存一份，事件送达后也要取走丢弃（discardPending）：否则暂存一直留着，
 * 页面一旦重载（WebView 重载、内容进程崩溃）就会把旧结果回放一遍。
 * 事件若恰好落在“挂监听”与“取暂存”之间，同一个结果会经两条路各到一次：
 * 记下从暂存回放的那份，挂监听后收到的第一个事件与它相同就跳过
 */
export function createCaptureDelivery<T>(handlers: {
    apply: (payload: T) => void | Promise<void>;
    discardPending: () => void;
    isSame: (a: T, b: T) => boolean;
}) {
    let eventSeen = false;
    let replayed: T | null = null;
    return {
        /** 事件送达 */
        onEvent(payload: T): void {
            handlers.discardPending();
            const previous = replayed;
            replayed = null;
            const isFirstEvent = !eventSeen;
            eventSeen = true;
            if (isFirstEvent && previous != null && handlers.isSame(previous, payload)) {
                return;
            }
            void handlers.apply(payload);
        },
        /** 取回的暂存（挂好监听之后调用） */
        async onPending(pending: T | null): Promise<void> {
            // 已经收到过事件时，暂存里的不会比它新（暂存只保留最近一次），交给事件即可
            if (pending != null && !eventSeen) {
                replayed = pending;
                await handlers.apply(pending);
            }
        },
    };
}

/**
 * 在分词结果中尽力标记捕获的单词。首选按选区偏移标记相交词元（同一单词在句中多次出现
 * 时也能标中真正被选中的那一处），偏移缺失或与文本不符时退回字符串匹配：
 * 精确词元 → 忽略大小写 → 连续词元短语 → 复合词元内的子串（encoder 落在
 * encoder-decoder 这个词元上）。
 *
 * 都不匹配就不标记：标记只是位置提示，缺位不影响查询——捕获词已经直接写进搜索框。
 */
export function markCapturedWord(list: SentenceToken[], word: string, wordOffset: number | null, sentence: string): void {
    const lower = word.toLowerCase();
    if (wordOffset != null && markTokensInRange(list, sentence, wordOffset, word)) {
        return;
    }
    // 单词匹配：精确，失败再忽略大小写
    let index = list.findIndex(t => t.token === word);
    if (index < 0) {
        index = list.findIndex(t => t.token.toLowerCase() === lower);
    }
    if (index >= 0) {
        list[index].marked = true;
        return;
    }
    // 短语匹配：按空白切分后，在词元下标序列上找连续的一段与各部分忽略大小写相等
    const parts = word.split(/\s+/).filter(part => part.length > 0);
    if (parts.length > 1) {
        const wordIndexes = list
            .map((t, i) => ({ token: t.token, index: i }))
            .filter(t => isWord(t.token))
            .map(t => t.index);
        outer: for (let start = 0; start + parts.length <= wordIndexes.length; start++) {
            for (let offset = 0; offset < parts.length; offset++) {
                if (list[wordIndexes[start + offset]].token.toLowerCase() !== parts[offset].toLowerCase()) {
                    continue outer;
                }
            }
            for (let offset = 0; offset < parts.length; offset++) {
                list[wordIndexes[start + offset]].marked = true;
            }
            return;
        }
    }
    // 复合词元内的子串选区：分词把词内连字符与撇号并入词元（encoder-decoder、don't 是
    // 单个词元，见 stringutils.ts），而取词可能只选中其中一段。按词元内的连接符切段后
    // 整段相等才命中（不做任意子串匹配，避免 code 命中 encoder），命中则标记整个词元
    const partIndex = list.findIndex(t => t.token.toLowerCase().split(/['’-]/).includes(lower));
    if (partIndex >= 0) {
        list[partIndex].marked = true;
    }
}

/**
 * 按选区区间标记词元：标记所有与 [offset, offset + word.length) 相交的单词词元——选区
 * 可能只是复合词元的一部分（encoder 在 encoder-decoder 内），也可能横跨多个词元。
 * 偏移与文本不一致（被裁剪或改写）时返回 false，由调用方回退到字符串匹配。
 */
function markTokensInRange(
    list: SentenceToken[],
    sentence: string,
    offset: number,
    word: string
): boolean {
    const end = offset + word.length;
    if (offset < 0 || end > sentence.length) {
        return false;
    }
    if (sentence.slice(offset, end).toLowerCase() !== word.toLowerCase()) {
        return false;
    }
    let position = 0;
    let marked = false;
    for (const item of list) {
        const start = position;
        position += item.token.length;
        if (position <= offset || start >= end) {
            continue;
        }
        if (isWord(item.token)) {
            item.marked = true;
            marked = true;
        }
    }
    return marked;
}

/** 划词未录入内容时的提示（按失败种类与平台给出引导） */
export function captureFailureDialog(failure: CaptureFailure, osType: string): { title: string; message: string; } {
    // 两种提示都以操作失败处理（ContentDialog 的 kind 只有 error/warning 两级，
    // 而 warning 是给破坏性操作确认用的）。
    //
    // empty：目标应用里没有可取用的选区——真的没选中文字，或应用不向无障碍接口
    // 暴露选区（Windows 上部分 UWP 应用如此；macOS 上如 VSCode 的编辑器）。
    // 托盘菜单路径下的取词目标是“用户最近所在的应用窗口”，所以还要提示先点回目标应用。
    //
    // failed：无障碍查询与模拟复制都没拿到内容——Windows 上常见于以管理员身份
    // 运行的窗口（UIPI 拦截注入）；macOS 上多为辅助功能未授权；Linux 上为 AT-SPI
    // 未暴露选区且 PRIMARY 也为空。
    const isMacOS = osType === 'macos';
    const isWindows = osType === 'windows';
    let message: string;
    if (failure === 'empty') {
        message = isMacOS
            ? '取词目标里没有可取用的选区：可能没有选中文字，也可能该应用不向辅助功能接口暴露选区（如 VSCode 的编辑器）。请选中文字后重试。'
            : isWindows
                ? '取词目标里没有可取用的选区：可能没有选中文字，也可能该应用不向无障碍接口暴露选区（部分 UWP 应用如此）。\n\n从托盘菜单触发时，取词目标是“你最近所在的应用窗口”，请先点击要取词的应用窗口再重试。'
                : '取词目标里没有可取用的选区：可能没有选中文字，也可能该应用未通过 AT-SPI 暴露选区；GNOME Wayland 下也不支持读取 PRIMARY 选区。请选中文字后重试。';
    } else {
        message = isMacOS
            ? '获取选中文本失败。\n\n请在“系统设置 → 隐私与安全性 → 辅助功能”中允许本应用，然后重试。'
            : isWindows
                ? '获取选中文本失败。\n\n目标应用既没有通过 UI Automation 暴露选区，也没有响应模拟的 Ctrl+C 复制（常见于以管理员身份运行的窗口，请换用普通权限的窗口重试）。若从托盘菜单触发，请先点击目标应用窗口再重试。'
                : '获取选中文本失败。\n\n目标应用没有通过 AT-SPI 暴露选区，也没能读到 PRIMARY 选区（请确认已选中文本；GNOME Wayland 下不支持读取 PRIMARY）。';
    }
    const title = failure === 'empty' ? '没有取到文本' : '划词录入失败';
    return { title, message };
}
