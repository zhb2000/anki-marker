/** 就地反馈的语义级别，配色/图标对应 WinUI 的 SystemFillColor* 系列 */
export type StatusSeverity = 'informational' | 'success' | 'warning' | 'error';

/** 容器内（卡片等）展示的一条就地反馈 */
export interface StatusNotice {
    severity: StatusSeverity;
    message: string;
    /** 文字下方显示不确定进度条，用于进行中的状态 */
    progress?: boolean;
    /** 文案后的操作按钮（如失败提示的“重试”） */
    actionLabel?: string;
}
