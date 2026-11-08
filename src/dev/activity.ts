/**
 * dev 夹具的“进行中”计数。
 *
 * 夹具在 IPC 之前就返回了结果，不会经过 installInvokeTrace 的 invoke 计数；
 * idle() 若不把这段活动算进去，会在 AI 流式输出、模拟延迟还没结束时误判“应用已安静”。
 */
let pending = 0;

/** 标记一段夹具活动开始，返回结束函数；重复调用结束函数只生效一次 */
export function beginMockActivity(): () => void {
    pending++;
    let ended = false;
    return () => {
        if (!ended) {
            ended = true;
            pending--;
        }
    };
}

/** 仍在进行中的夹具活动数量 */
export function pendingMockActivity(): number {
    return pending;
}
