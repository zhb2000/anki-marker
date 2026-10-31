import type { StatusNotice } from '../../fluent-controls/status';
import type { ItemModel } from './types';

/** 操作超过这个时长才显示进度与“停止”：正常速度的添加/移除不应闪一下 */
const SLOW_NOTICE_DELAY_MS = 700;

/** 词典卡片的就地反馈：文案写在条目的 notice 上，计时器与操作回调按条目对象持有 */
export interface CardNotices {
    /**
     * 给某张卡片挂一条就地反馈。
     * duration > 0 时到点自动收起（触发 FluentStatusText 的退场动画）；0 表示常驻，
     * 由调用方在流程结束处 clear。
     */
    show(item: ItemModel<unknown>, notice: StatusNotice, duration?: number, onAction?: () => void): void;
    /** 收起某张卡片的就地反馈（进行中流程结束时调用） */
    clear(item: ItemModel<unknown>): void;
    /** 点击卡片提示上的操作（停止 / 重试） */
    runAction(item: ItemModel<unknown>): void;
}

export function createCardNotices(): CardNotices {
    /** 卡片就地反馈的自动收起计时器（按条目对象持有，条目被替换时自然失效） */
    const timers = new WeakMap<object, number>();
    /** 卡片就地反馈上的操作回调（“停止”“重试”），随提示一起替换 */
    const actions = new WeakMap<object, () => void>();

    function show(item: ItemModel<unknown>, notice: StatusNotice, duration = 3000, onAction?: () => void): void {
        const timer = timers.get(item);
        if (timer != null) {
            window.clearTimeout(timer);
            timers.delete(item);
        }
        item.notice = notice;
        if (onAction != null) {
            actions.set(item, onAction);
        } else {
            actions.delete(item);
        }
        if (duration > 0) {
            timers.set(item, window.setTimeout(() => { item.notice = null; }, duration));
        }
    }

    function clear(item: ItemModel<unknown>): void {
        const timer = timers.get(item);
        if (timer != null) {
            window.clearTimeout(timer);
            timers.delete(item);
        }
        actions.delete(item);
        item.notice = null;
    }

    function runAction(item: ItemModel<unknown>): void {
        actions.get(item)?.();
    }

    return { show, clear, runAction };
}

/**
 * 一次加卡/移除操作的取消句柄。
 *
 * - 前 700ms 不显示任何提示：正常速度的添加/移除不应闪一下；
 * - 超过阈值才出现“进度 + 停止”，点停止后界面立即退出等待；
 * - signal 交给可在 HTTP 层中断的请求（删除）；无法中断的等待（共享的 Anki 启动、
 *   准备牌组）用 race 退出——共享启动不会因为某一张卡片停止而被打断，只解除本卡片的等待；
 * - disableStop 之后不再提供“停止”（写入笔记阶段，见 addCardToAnki）。
 */
export function createCardOperation(notices: CardNotices, item: ItemModel<unknown>, initialMessage: string) {
    const controller = new AbortController();
    let rejectCancelled: ((reason?: unknown) => void) | null = null;
    const cancelledPromise = new Promise<never>((_, reject) => { rejectCancelled = reject; });
    let stopped = false;
    let stoppable = true;
    let noticeVisible = false;
    let slowTimer: number | null = null;

    const clearSlowTimer = (): void => {
        if (slowTimer != null) {
            window.clearTimeout(slowTimer);
            slowTimer = null;
        }
    };

    const stop = (): void => {
        if (stopped || !stoppable) {
            return;
        }
        stopped = true;
        clearSlowTimer();
        controller.abort();
        rejectCancelled?.(new Error('操作已停止'));
    };

    const showProgress = (message: string): void => {
        if (stopped) {
            return;
        }
        clearSlowTimer();
        noticeVisible = true;
        if (stoppable) {
            notices.show(item, { severity: 'informational', message, progress: true, actionLabel: '停止' }, 0, stop);
        } else {
            notices.show(item, { severity: 'informational', message, progress: true }, 0);
        }
    };

    /** 仅在提示已经可见时更新文案：避免快操作因为一句“正在写入…”闪一下 */
    const updateProgress = (message: string): void => {
        if (noticeVisible) {
            showProgress(message);
        }
    };

    /**
     * 进入不可中止的阶段，并（在提示已可见时）把文案换成 message、去掉“停止”。
     * 已经停止时返回 false，调用方不应再继续
     */
    const disableStop = (message: string): boolean => {
        if (stopped) {
            return false;
        }
        stoppable = false;
        updateProgress(message);
        return true;
    };

    slowTimer = window.setTimeout(() => { slowTimer = null; showProgress(initialMessage); }, SLOW_NOTICE_DELAY_MS);

    return {
        signal: controller.signal,
        get stopped() { return stopped; },
        race: <T>(promise: Promise<T>): Promise<T> => Promise.race([promise, cancelledPromise]),
        showProgress,
        updateProgress,
        disableStop,
        dispose: clearSlowTimer,
    };
}
