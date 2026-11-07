import { computed, nextTick, ref, useTemplateRef } from 'vue';

import * as api from '../../tauri-api';
import { tokenize } from '../../logics/stringutils';
import { useTransientNotice } from '../../fluent-controls/useTransientNotice';
import { rebuildDraft, tokensToSentenceHTML } from './sentenceDraft';
import { selectionForCapturedWord, type CapturedContextPayload } from './captureLogic';
import type { SentenceToken, TokenSelection } from './types';

export interface UseSentenceEditorOptions {
    /**
     * 确认编辑时本次编辑会话改过句子（语境变了）：由调用方决定是否按同一查询词重新取释义
     */
    onSentenceEdited: () => void;
}

/** 句子与分词，以及文本框句子编辑 */
export function useSentenceEditor(options: UseSentenceEditorOptions) {
    /** 划词面板的词元 */
    const tokens = ref<SentenceToken[]>([]);
    /** 划词面板的选区（查询目标）：恒为一段连续词元，null 表示没有选中任何词 */
    const selection = ref<TokenSelection | null>(null);
    /** 文本框中的句子 */
    const sentence = ref('');
    /** 是否显示文本框 */
    const showEdit = ref(false);
    /**
     * 句子两侧的上下文（近端在前）。语义上属于“当前这次捕获的初始状态”：
     * 任何整体替换（划词、粘贴、编辑确认）都会把它作废
     */
    const contextBefore = ref<string[]>([]);
    const contextAfter = ref<string[]>([]);

    /** 句子状态快照：撤销/恢复把整份状态（文本、选区、两侧上下文）一起还原 */
    interface SentenceSnapshot {
        text: string;
        selection: TokenSelection | null;
        before: string[];
        after: string[];
    }
    /** 撤销栈与恢复栈（栈顶是最近一次）。上限防止长时间使用后无限增长 */
    const undoStack = ref<SentenceSnapshot[]>([]);
    const redoStack = ref<SentenceSnapshot[]>([]);
    const canUndo = computed(() => undoStack.value.length > 0);
    const canRedo = computed(() => redoStack.value.length > 0);
    const MAX_HISTORY = 50;

    function snapshot(): SentenceSnapshot {
        return {
            text: sentence.value,
            selection: selection.value == null ? null : { ...selection.value },
            before: [...contextBefore.value],
            after: [...contextAfter.value],
        };
    }

    /** 在改动句子之前调用：记下当前状态，并作废恢复栈（新动作会分叉历史） */
    function pushHistory(state: SentenceSnapshot = snapshot()): void {
        undoStack.value.push(state);
        if (undoStack.value.length > MAX_HISTORY) {
            undoStack.value.shift();
        }
        redoStack.value = [];
    }

    function restore(state: SentenceSnapshot): void {
        sentence.value = state.text;
        tokens.value = tokenize(state.text).map(token => ({ token }));
        selection.value = state.selection == null ? null : { ...state.selection };
        contextBefore.value = [...state.before];
        contextAfter.value = [...state.after];
        // 还原后不再有进行中的编辑草稿
        editBaselineTokens = null;
        editBaselineSelection = null;
        editSentenceDirty = false;
    }

    /** 撤销：成功返回 true（调用方据此重取释义） */
    function undo(): boolean {
        const state = undoStack.value.pop();
        if (state == null) {
            return false;
        }
        redoStack.value.push(snapshot());
        restore(state);
        return true;
    }

    /** 恢复 */
    function redo(): boolean {
        const state = redoStack.value.pop();
        if (state == null) {
            return false;
        }
        undoStack.value.push(snapshot());
        restore(state);
        return true;
    }
    /** 文本框控件（模板中 ref="editTextArea"；聚焦经组件暴露的 focus 方法转发到内部 textarea） */
    const editTextArea = useTemplateRef<{ focus: () => void; }>('editTextArea');
    const isMacOS = computed(() => api.os.type() === 'macos');

    /** 根据平台显示不同的快捷键提示 */
    const editPlaceholder = computed(() => {
        return isMacOS.value
            ? '⌘ + Enter 完成编辑'
            : 'Ctrl + Enter 完成编辑';
    });

    /**
     * 编辑会话的基线（分词与选区）：进入编辑模式时快照，确认编辑时以此为准重建。
     *
     * 编辑框中的内容视为草稿：编辑期间逐键输入不重建分词（避免把“拆开单词”的
     * 中间状态固化下来，导致复原后选区无从恢复），确认（完成）时才一次性以基线
     * 为基准重建，尽量保留已选中的词。整体替换句子（划词录入、粘贴）会作废草稿。
     */
    let editBaselineTokens: SentenceToken[] | null = null;
    let editBaselineSelection: TokenSelection | null = null;
    /** 进入编辑模式时的句子文本：确认编辑时作为撤销快照 */
    let editBaselineText = '';
    /**
     * 本次编辑会话中句子是否被改动过：确认编辑时据此决定要不要按新语境重取释义。
     * 用“改过”而不是“前后文本不同”——改完又改回去时文本相同，但旧释义已被清空，仍应恢复
     */
    let editSentenceDirty = false;

    /**
     * 程序化整体替换句子（划词录入、粘贴、dev 测试句等）：
     * 句子与分词同步重建为全新状态（清空选区），并作废进行中的编辑草稿。
     * 值相同也重建——每次整体替换都应是全新状态。
     */
    function replaceSentence(text: string): void {
        contextBefore.value = [];
        contextAfter.value = [];
        // 整体替换是“新的一次录入”：旧句子的撤销历史不再有意义
        undoStack.value = [];
        redoStack.value = [];
        resetSentence(text);
    }

    /** 整体替换句子但不碰建议（建议并入自己用：并入后队首推进到下一档） */
    function resetSentence(text: string): void {
        editBaselineTokens = null;
        editBaselineSelection = null;
        editSentenceDirty = false;
        if (sentence.value !== text) {
            sentence.value = text;
        }
        tokens.value = tokenize(text).map(token => ({ token }));
        selection.value = null;
    }

    /** 捕获结果带来的上下文（两侧都可以为空） */
    function setContext(context: CapturedContextPayload): void {
        contextBefore.value = context.before;
        contextAfter.value = context.after;
    }

    /**
     * 并入一侧的上下文：把该侧最近的一句接到句子前面 / 后面。
     * 分词同步重建；标记词按平移后的位置重新标出（上文并入会让词整体右移）。
     * 返回并入后的句子；该侧没有更多文本时返回 null
     */
    function extendContext(side: 'before' | 'after'): string | null {
        const queue = side === 'before' ? contextBefore : contextAfter;
        const piece = queue.value[0];
        if (piece == null) {
            return null;
        }
        pushHistory();
        const marked = selection.value == null
            ? ''
            : tokens.value
                .slice(selection.value.start, selection.value.end + 1)
                .map(token => token.token)
                .join(' ');
        const wordOffset = selection.value == null
            ? null
            : tokens.value
                .slice(0, selection.value.start)
                .reduce((offset, token) => offset + token.token.length, 0);
        const text = side === 'before' ? piece + ' ' + sentence.value : sentence.value + ' ' + piece;
        const shift = side === 'before' ? piece.length + 1 : 0;
        queue.value = queue.value.slice(1);
        resetSentence(text);
        if (marked.length > 0) {
            selection.value = selectionForCapturedWord(
                tokens.value, marked, wordOffset == null ? null : wordOffset + shift, text
            );
        }
        return text;
    }

    /** 编辑框的输入：标记本次编辑会话确实改过句子 */
    function markSentenceDirty(): void {
        editSentenceDirty = true;
    }

    /** 粘贴失败的就地提示：显示在句子面板上方，紧挨着“粘贴”按钮 */
    const { notice: pasteNotice, show: showPasteNotice } = useTransientNotice(3000);

    /**
     * 读取要粘贴为句子的剪贴板文本（已裁剪首尾空白）。
     * 剪贴板里没有可用文本时就地提示并返回 null，调用方不应替换句子
     */
    async function readPastedSentence(): Promise<string | null> {
        let text: string;
        try {
            text = await api.clipboard.readText();
        } catch (error) {
            // 剪贴板为空或内容不是文本（如图片）时，剪贴板插件直接报错而不是返回空串
            console.error(error);
            showPasteNotice('剪贴板中没有文本', { severity: 'error' });
            return null;
        }
        const trimmed = text.trim();
        if (trimmed.length === 0) {
            // 只有空白：不替换句子，避免一次误粘贴清掉当前工作区
            showPasteNotice('剪贴板中没有文本', { severity: 'error' });
            return null;
        }
        return trimmed;
    }

    async function changeEditStatus() {
        showEdit.value = !showEdit.value;
        if (showEdit.value) {
            // 进入编辑模式：快照当前分词与选区作为基线，编辑期间不再重建分词；清空“本次编辑是否改过句子”
            editBaselineTokens = tokens.value.map(({ token }) => ({ token }));
            editBaselineSelection = selection.value;
            editBaselineText = sentence.value;
            editSentenceDirty = false;
            await nextTick();
            editTextArea.value?.focus();
        } else {
            // 首尾空白在确认时才裁剪：编辑期间逐键裁剪会吞掉句末刚打的空格（见 fluent-controls/useTextModel.ts）
            const trimmed = sentence.value.trim();
            if (trimmed !== sentence.value) {
                sentence.value = trimmed;
            }
            // 确认编辑（完成）：以基线分词与选区为基准重建，尽量保留已选中的词。
            // 基线为 null 说明编辑期间句子被整体替换（划词录入、粘贴），分词已是全新状态，无需重建
            if (editBaselineTokens != null) {
                if (editSentenceDirty) {
                    pushHistory({
                        text: editBaselineText,
                        selection: editBaselineSelection,
                        before: [...contextBefore.value],
                        after: [...contextAfter.value],
                    });
                }
                const rebuilt = rebuildDraft(editBaselineTokens, editBaselineSelection, sentence.value);
                tokens.value = rebuilt.tokens;
                selection.value = rebuilt.selection;
                editBaselineTokens = null;
                editBaselineSelection = null;
                // 句子改过 = 语境变了，交给调用方按新语境重取释义；没改动过则什么都不做
                if (editSentenceDirty) {
                    options.onSentenceEdited();
                }
            }
            editSentenceDirty = false;
        }
    }

    /** 处理键盘事件，支持 macOS 的 Cmd + Enter 和其他平台的 Ctrl + Enter */
    async function handleEditTextAreaKeydown(event: KeyboardEvent) {
        const isCorrectModifier = isMacOS.value ? event.metaKey : event.ctrlKey;
        if (isCorrectModifier && event.key === 'Enter') {
            event.preventDefault();
            await changeEditStatus();
        }
    }

    /**
     * 生成卡片例句的 HTML（选区内的单词加粗）。
     *
     * 编辑中还没点“完成”时，用户看到的是编辑框里的草稿，而 tokens 在编辑期间刻意不重建、仍是编辑前的句子：
     * 此时按确认编辑的同一规则从草稿生成分词与选区，加卡内容与用户所见一致，且不打断编辑
     */
    function makeSentenceHTML(): string {
        if (showEdit.value && editBaselineTokens != null) {
            const rebuilt = rebuildDraft(editBaselineTokens, editBaselineSelection, sentence.value);
            return tokensToSentenceHTML(rebuilt.tokens, rebuilt.selection);
        }
        return tokensToSentenceHTML(tokens.value, selection.value);
    }

    return {
        tokens,
        selection,
        sentence,
        showEdit,
        editPlaceholder,
        pasteNotice,
        contextBefore,
        contextAfter,
        canUndo,
        canRedo,
        setContext,
        extendContext,
        undo,
        redo,
        replaceSentence,
        markSentenceDirty,
        readPastedSentence,
        changeEditStatus,
        handleEditTextAreaKeydown,
        makeSentenceHTML,
    };
}

export type SentenceEditor = ReturnType<typeof useSentenceEditor>;
