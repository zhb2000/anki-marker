import { computed, nextTick, ref, useTemplateRef } from 'vue';

import * as api from '../../tauri-api';
import { tokenize } from '../../logics/stringutils';
import { useTransientNotice } from '../../fluent-controls/useTransientNotice';
import { rebuildDraftTokens, tokensToSentenceHTML } from './sentenceDraft';
import type { SentenceToken } from './types';

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
    /** 文本框中的句子 */
    const sentence = ref('');
    /** 是否显示文本框 */
    const showEdit = ref(false);
    /** 文本框控件（模板中 ref="editTextArea"；聚焦经组件暴露的 focus 方法转发到内部 textarea） */
    const editTextArea = useTemplateRef<{ focus: () => void }>('editTextArea');
    const isMacOS = computed(() => api.os.type() === 'macos');

    /** 根据平台显示不同的快捷键提示 */
    const editPlaceholder = computed(() => {
        return isMacOS.value
            ? '⌘ + Enter 完成编辑'
            : 'Ctrl + Enter 完成编辑';
    });

    /**
     * 编辑会话的基线分词：进入编辑模式时快照，确认编辑时以此为准重建分词。
     *
     * 编辑框中的内容视为草稿：编辑期间逐键输入不重建分词（避免把“拆开单词”的
     * 中间状态固化下来，导致复原后标记无从恢复），确认（完成）时才一次性以基线
     * 为基准重建，尽量保留已标记单词。整体替换句子（划词录入、粘贴）会作废草稿。
     */
    let editBaselineTokens: SentenceToken[] | null = null;
    /**
     * 本次编辑会话中句子是否被改动过：确认编辑时据此决定要不要按新语境重取释义。
     * 用“改过”而不是“前后文本不同”——改完又改回去时文本相同，但旧释义已被清空，仍应恢复
     */
    let editSentenceDirty = false;

    /**
     * 程序化整体替换句子（划词录入、粘贴、dev 测试句等）：
     * 句子与分词同步重建为全新状态（清除已有标记），并作废进行中的编辑草稿。
     * 值相同也重建——每次整体替换都应是全新状态。
     */
    function replaceSentence(text: string): void {
        editBaselineTokens = null;
        editSentenceDirty = false;
        if (sentence.value !== text) {
            sentence.value = text;
        }
        tokens.value = tokenize(text).map(token => ({ token, marked: false }));
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
            // 进入编辑模式：快照当前分词作为基线，编辑期间不再重建分词；清空“本次编辑是否改过句子”
            editBaselineTokens = tokens.value.map(({ token, marked }) => ({ token, marked }));
            editSentenceDirty = false;
            await nextTick();
            editTextArea.value?.focus();
        } else {
            // 首尾空白在确认时才裁剪：编辑期间逐键裁剪会吞掉句末刚打的空格（见 fluent-controls/useTextModel.ts）
            const trimmed = sentence.value.trim();
            if (trimmed !== sentence.value) {
                sentence.value = trimmed;
            }
            // 确认编辑（完成）：以基线分词为基准重建，尽量保留已标记单词。
            // 基线为 null 说明编辑期间句子被整体替换（划词录入、粘贴），分词已是全新状态，无需重建
            if (editBaselineTokens != null) {
                tokens.value = rebuildDraftTokens(editBaselineTokens, sentence.value);
                editBaselineTokens = null;
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
     * 生成卡片例句的 HTML（标记的词加粗）。
     *
     * 编辑中还没点“完成”时，用户看到的是编辑框里的草稿，而 tokens 在编辑期间刻意不重建、仍是编辑前的句子：
     * 此时按确认编辑的同一规则从草稿生成分词，加卡内容与用户所见一致，且不打断编辑
     */
    function makeSentenceHTML(): string {
        const source = (showEdit.value && editBaselineTokens != null)
            ? rebuildDraftTokens(editBaselineTokens, sentence.value)
            : tokens.value;
        return tokensToSentenceHTML(source);
    }

    return {
        tokens,
        sentence,
        showEdit,
        editPlaceholder,
        pasteNotice,
        replaceSentence,
        markSentenceDirty,
        readPastedSentence,
        changeEditStatus,
        handleEditTextAreaKeydown,
        makeSentenceHTML,
    };
}

export type SentenceEditor = ReturnType<typeof useSentenceEditor>;
