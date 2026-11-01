import { escapeHTML, isWord, rebuildTokensPreservingMarks } from '../../logics/stringutils';
import { selectionContains, selectionFromMarks } from './selection';
import type { SentenceToken, TokenSelection } from './types';

/** 从编辑草稿重建的结果：分词与随句子一起迁移过来的选区 */
export interface DraftRebuild {
    tokens: SentenceToken[];
    selection: TokenSelection | null;
}

/**
 * 按“确认编辑”的规则从编辑草稿重建分词与选区：首尾空白在确认时才裁剪，再以进入编辑时的
 * 基线为基准重建，尽量保留已选中的词。确认编辑与“编辑中途加卡”共用这条规则，
 * 加卡内容才与确认后用户看到的句子一致
 *
 * 复用“按词元对齐迁移标记”的既有逻辑（LCS + 模糊对齐）：先把选区摊平成词元标记，
 * 对齐后再收拢回一段连续区间——选区模型不允许出现两段
 */
export function rebuildDraft(
    baseline: SentenceToken[],
    baselineSelection: TokenSelection | null,
    draft: string
): DraftRebuild {
    const marked = baseline.map(({ token }, index) => ({
        token,
        marked: selectionContains(baselineSelection, index),
    }));
    const rebuilt = rebuildTokensPreservingMarks(marked, draft.trim());
    return {
        tokens: rebuilt.map(({ token }) => ({ token })),
        selection: selectionFromMarks(rebuilt),
    };
}

/** 生成卡片例句的 HTML：逐词元转义，选区内的单词加粗 */
export function tokensToSentenceHTML(tokens: SentenceToken[], selection: TokenSelection | null): string {
    return tokens.map(({ token }, index) => {
        const bold = selectionContains(selection, index) && isWord(token);
        return bold ? `<b>${escapeHTML(token)}</b>` : escapeHTML(token);
    }).join('');
}
