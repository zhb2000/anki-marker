import { escapeHTML, rebuildTokensPreservingMarks } from '../../logics/stringutils';
import type { SentenceToken } from './types';

/**
 * 按“确认编辑”的规则从编辑草稿重建分词：首尾空白在确认时才裁剪，再以进入编辑时的基线分词
 * 为基准重建，尽量保留已标记单词。确认编辑与“编辑中途加卡”共用这条规则，
 * 加卡内容才与确认后用户看到的句子一致
 */
export function rebuildDraftTokens(baseline: SentenceToken[], draft: string): SentenceToken[] {
    return rebuildTokensPreservingMarks(baseline, draft.trim());
}

/** 生成卡片例句的 HTML：逐词元转义，标记的词加粗 */
export function tokensToSentenceHTML(tokens: SentenceToken[]): string {
    return tokens.map(
        ({ token, marked }) => marked
            ? `<b>${escapeHTML(token)}</b>`
            : escapeHTML(token)
    ).join('');
}
