export class AsyncLock {
    private isLocked: boolean;
    private waitingResolvers: Array<() => void>;

    constructor() {
        this.isLocked = false;
        this.waitingResolvers = [];
    }

    /** 检查锁是否被持有 */
    public locked(): boolean {
        return this.isLocked;
    }

    /** 尝试获取锁 */
    public async acquire(): Promise<void> {
        // 如果锁已被持有，等待直到锁被释放
        if (this.isLocked) {
            await new Promise<void>(resolve => {
                this.waitingResolvers.push(resolve);
            });
        }
        // 获取锁
        this.isLocked = true;
    }

    /** 释放锁 */
    public release() {
        // 如果队列中有等待的操作，唤醒下一个
        if (this.waitingResolvers.length > 0) {
            const resolve = this.waitingResolvers.shift();
            if (resolve != null) {
                resolve();
            }
        } else {
            // 没有等待的操作，释放锁
            this.isLocked = false;
        }
    }
}
