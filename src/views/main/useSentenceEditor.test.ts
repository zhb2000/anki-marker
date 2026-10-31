import { beforeEach, describe, expect, it, vi } from 'vitest';

import { decodeHtmlEntities } from '../../logics/stringutils';
import { useSentenceEditor } from './useSentenceEditor';

const mocks = vi.hoisted(() => ({
    readText: vi.fn<() => Promise<string>>(),
}));

vi.mock('../../tauri-api', () => ({
    os: { type: () => 'macos' },
    clipboard: { readText: mocks.readText },
}));

function setup() {
    const onSentenceEdited = vi.fn<() => void>();
    const editor = useSentenceEditor({ onSentenceEdited });
    editor.replaceSentence('The quick brown fox jumps.');
    // 用户标记了 fox
    editor.tokens.value[6].marked = true;
    return { editor, onSentenceEdited };
}

function joined(editor: ReturnType<typeof useSentenceEditor>): string {
    return editor.tokens.value.map(({ token }) => token).join('');
}

function marked(editor: ReturnType<typeof useSentenceEditor>): string[] {
    return editor.tokens.value.filter(t => t.marked).map(t => t.token);
}

beforeEach(() => {
    vi.clearAllMocks();
    // 测试中没有组件实例，useTemplateRef 会告警
    vi.spyOn(console, 'warn').mockImplementation(() => { });
});

describe('useSentenceEditor', () => {
    it('编辑期间草稿原样保留（含句末空格），分词不随逐键输入重建', async () => {
        const { editor } = setup();
        await editor.changeEditStatus();
        editor.sentence.value = 'The quick brown fox jumps over ';
        editor.markSentenceDirty();
        expect(editor.sentence.value).toBe('The quick brown fox jumps over ');
        expect(joined(editor)).toBe('The quick brown fox jumps.');
    });

    it('编辑中途加卡：例句按确认规则从草稿生成，与确认后所见一致', async () => {
        const { editor } = setup();
        await editor.changeEditStatus();
        editor.sentence.value = '  The quick <brown> fox jumps over  ';
        const html = editor.makeSentenceHTML();
        expect(html).toBe('The quick &lt;brown&gt; <b>fox</b> jumps over');
        expect(decodeHtmlEntities(html.replace(/<\/?b>/g, ''))).toBe('The quick <brown> fox jumps over');
        await editor.changeEditStatus();
        expect(editor.makeSentenceHTML()).toBe(html);
    });

    it('确认编辑：裁剪首尾空白，拼回文本与草稿一致，标记保留，并通知句子改过', async () => {
        const { editor, onSentenceEdited } = setup();
        await editor.changeEditStatus();
        editor.sentence.value = 'Yesterday the quick brown fox jumps high.  ';
        editor.markSentenceDirty();
        await editor.changeEditStatus();
        expect(editor.showEdit.value).toBe(false);
        expect(editor.sentence.value).toBe('Yesterday the quick brown fox jumps high.');
        expect(joined(editor)).toBe('Yesterday the quick brown fox jumps high.');
        expect(marked(editor)).toEqual(['fox']);
        expect(onSentenceEdited).toHaveBeenCalledOnce();
    });

    it('没改过句子就确认：不通知', async () => {
        const { editor, onSentenceEdited } = setup();
        await editor.changeEditStatus();
        await editor.changeEditStatus();
        expect(marked(editor)).toEqual(['fox']);
        expect(onSentenceEdited).not.toHaveBeenCalled();
    });

    it('编辑期间整体替换句子：草稿作废，确认时不再按旧基线重建、也不通知', async () => {
        const { editor, onSentenceEdited } = setup();
        await editor.changeEditStatus();
        editor.markSentenceDirty();
        editor.replaceSentence('A brand new fox.');
        await editor.changeEditStatus();
        expect(joined(editor)).toBe('A brand new fox.');
        expect(marked(editor)).toEqual([]);
        expect(onSentenceEdited).not.toHaveBeenCalled();
    });

    it('粘贴：剪贴板只有空白或读取失败时返回 null 并就地提示', async () => {
        const { editor } = setup();
        vi.spyOn(console, 'error').mockImplementation(() => { });
        mocks.readText.mockResolvedValueOnce('  \n ');
        expect(await editor.readPastedSentence()).toBe(null);
        expect(editor.pasteNotice.value?.message).toBe('剪贴板中没有文本');
        mocks.readText.mockRejectedValueOnce(new Error('not text'));
        expect(await editor.readPastedSentence()).toBe(null);
        mocks.readText.mockResolvedValueOnce('  Pasted sentence.\n');
        expect(await editor.readPastedSentence()).toBe('Pasted sentence.');
    });
});
