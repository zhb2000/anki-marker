import { onScopeDispose, ref } from 'vue';

import type { StatusNotice, StatusSeverity } from './status';

export interface TransientNoticeOptions {
    severity?: StatusSeverity;
    /** 自动收起毫秒数，0 表示常驻直到调用方 clear() */
    duration?: number;
}

/**
 * 就地反馈的状态与计时器。
 *
 * 就地反馈没有全局队列，每条消息由所在控件自己持有：show() 写入文案并开始计时，
 * 到点自动清空（清空即触发 FluentStatusText 的退场动画）。反馈的空间本来就属于
 * 所在容器，因此不需要“互斥/排队”这类全局约束。
 */
export function useTransientNotice(defaultDuration = 3000) {
    const notice = ref<StatusNotice | null>(null);
    let timer: number | null = null;

    function clear(): void {
        if (timer != null) {
            window.clearTimeout(timer);
            timer = null;
        }
        notice.value = null;
    }

    function show(message: string, options: TransientNoticeOptions = {}): void {
        const { severity = 'informational', duration = defaultDuration } = options;
        if (timer != null) {
            window.clearTimeout(timer);
        }
        notice.value = { severity, message };
        timer = duration > 0 ? window.setTimeout(() => { notice.value = null; timer = null; }, duration) : null;
    }

    onScopeDispose(clear);

    return { notice, show, clear };
}
