import type { Ref } from 'vue';

import * as anki from '../../logics/anki';
import * as cfg from '../../logics/config';
import * as dict from '../../logics/dict';
import * as globals from '../../logics/globals';
import type * as aiPickLogic from '../../logics/aiPick';
import { dialog } from '../../fluent-controls';
import { createCardOperation, type CardNotices } from './cardNotices';
import type { DictName, ItemModel, WordItems } from './types';

/** 写入失败后核对笔记时，按创建时间筛选所留的时钟偏差余量（AnkiConnect 可能在另一台机器上） */
const WRITE_RECONCILE_CLOCK_SLACK_MS = 10_000;

/**
 * 一次加卡的全部写入内容，在第一个 await 之前一次性快照：之后可能要等 Anki 启动几十秒，
 * 期间用户可能又划了一句、编辑了句子，或得到了新词的 AI 优选（恰好落在同一词典的同一序号上），
 * 这些都不能混进这张卡片。加卡流程只读这份快照，不回读界面的当前状态
 */
export interface AddNoteSnapshot {
    dictionary: DictName;
    /** 所加的词典条目 */
    entry: dict.CollinsItem | dict.OxfordItem | dict.YoudaoItem;
    /** 卡片例句的 HTML（已转义，标记的词加粗） */
    sentenceHTML: string;
    /** 写入“笔记”字段的 AI 笔记；null 表示不写 */
    aiNote: string | null;
    pronunciationType: 'en' | 'us';
    deckName: string;
    modelName: string;
}

/** 加卡/移除流程的协作方 */
export interface CardActionContext {
    ankiService: anki.AnkiService;
    notices: CardNotices;
    /** 失败提示上“重试”按钮的动作 */
    retry: () => void;
}

/**
 * 写入“笔记”字段的 AI 笔记：仅当所加条目正是 AI 优选命中的条目时才带上。
 * 只取已完成的优选：流式输出中的笔记是半截的，提前解析出的 pick 也可能被最终结果推翻
 */
export function aiNoteFor(state: aiPickLogic.AiPickState, dictionary: DictName, index: number): string | null {
    const picked = state.pick != null && state.pick.source === dictionary && state.pick.index === index;
    return (state.phase === 'done' && picked) ? state.note : null;
}

/** 若牌组或笔记模板不存在，则创建之（signal 仅用于“已取消时不再弹错误框”） */
export async function prepareDeckAndModel(
    ankiService: anki.AnkiService,
    deckName: string,
    modelName: string,
    signal?: AbortSignal
) {
    let errorTitle: string | null = null;

    async function prepareDeck(deckName: string) {
        try {
            await ankiService.createDeck(deckName);
        } catch (error) {
            errorTitle = `牌组 ${deckName} 创建失败`;
            throw error;
        }
    }

    /**
     * @returns 是否创建了新的笔记模板
     */
    async function prepareModel(modelName: string): Promise<boolean> {
        let modelExists: boolean;
        try {
            const modelNames = await ankiService.modelNames();
            modelExists = modelNames.includes(modelName);
        } catch (error) {
            errorTitle = `查询笔记模板 ${modelName} 失败`;
            throw error;
        }
        if (!modelExists) {
            try {
                await ankiService.createMarkerModel(modelName);
            } catch (error) {
                errorTitle = `笔记模板 ${modelName} 创建失败`;
                throw error;
            }
            return true;
        }
        return false;
    }

    let newModelCreated: boolean;
    try {
        newModelCreated = (await Promise.all([prepareDeck(deckName), prepareModel(modelName)]))[1];
    } catch (error) {
        // 用户点了“停止”：这是预期内的中断，不再弹错误框
        if (signal?.aborted !== true) {
            await dialog.message(String(error), { title: errorTitle!, kind: 'error' });
        }
        throw error;
    }
    if (newModelCreated) {
        globals.templateVersion.value = anki.CARD_TEMPLATE_VERSION;
    } else {
        // 若模板版本号未获取，则此时尝试获取一次
        if (typeof globals.templateVersion.value !== 'string') {
            void globals.fetchAndSetTemplateVersion(modelName);
        }
    }
}

/** 把一张未添加的卡片加到 Anki：写入内容全部来自 snapshot */
export async function addCardToAnki(
    item: ItemModel<unknown>,
    snapshot: AddNoteSnapshot,
    context: CardActionContext
): Promise<void> {
    const { ankiService, notices, retry } = context;
    const { dictionary, entry, sentenceHTML, aiNote, pronunciationType, deckName, modelName } = snapshot;
    item.status = 'processing-add';
    const operation = createCardOperation(notices, item, '正在添加到 Anki…');
    // 出错或中断时先清掉进行中的提示，再决定显示什么（成功则什么都不显示）
    const settle = () => {
        operation.dispose();
        notices.clear(item);
    };
    let phase: 'connect' | 'prepare' | 'write' = 'connect';
    /** 写入请求是否已经发出：发出之后的失败不能直接当作“未添加” */
    let noteSent = false;
    let fields: ReturnType<typeof anki.makeFields> | null = null;
    let writeStartedAt = 0;
    try {
        // 确保 AnkiConnect 可用：不可用时按配置自动启动 Anki 并等待其就绪
        await operation.race(globals.ensureAnkiConnect(
            message => operation.showProgress(message),
            { signal: operation.signal },
        ));
        // 连接已就绪：文案切到“添加中”，不要再停在“启动/连接 Anki”
        operation.updateProgress('正在添加到 Anki…');
        phase = 'prepare';
        await operation.race(prepareDeckAndModel(ankiService, deckName, modelName, operation.signal));
        phase = 'write';
        // AI 优选联动：若所加条目正是 AI 优选命中的条目且带有笔记，则写入“笔记”字段
        fields = anki.makeFields(dictionary, entry, sentenceHTML, aiNote);
        const word = ('phrase' in entry && entry.phrase != null)
            ? entry.phrase
            : entry.word;
        // 发音地址全部不可用时不附带音频（见 addMarkerNote 的说明），不再把已判定不可用的地址交给 AnkiConnect
        const audioResult = await operation.race(dict.makePronunciationURL(word, pronunciationType));
        const audio = audioResult != null
            ? {
                url: audioResult.url,
                filename: await dict.makePronunciationFilename(word, pronunciationType, audioResult.dict),
            }
            : null;
        // 写入请求一旦发出，Anki 就会照常完成（含下载音频），前端放弃等待并不能撤回：
        // 从这里起不再提供“停止”，也不把请求挂在可中止的信号上
        if (!operation.disableStop('正在写入笔记…')) {
            throw new Error('操作已停止');
        }
        // 笔记 id 是创建时刻的毫秒时间戳，核对时据此只看本次写入之后创建的笔记；
        // 留出余量，容忍 AnkiConnect 在另一台机器上时的时钟偏差
        writeStartedAt = Date.now() - WRITE_RECONCILE_CLOCK_SLACK_MS;
        noteSent = true;
        const id = await ankiService.addMarkerNote(deckName, modelName, fields, audio);
        if (id == null) {
            throw new Error('addNote returns null');
        }
        item.id = id;
        item.status = 'is-added';
        settle();
    } catch (error) {
        const stopped = operation.stopped;
        operation.dispose();
        if (stopped) {
            // 用户主动停止：回到未添加，给一句短提示说明“确实停了”
            item.status = 'not-added';
            notices.clear(item);
            notices.show(item, { severity: 'informational', message: '已停止添加' }, 1500);
            return;
        }
        console.error(error);
        if (phase === 'connect') {
            item.status = 'not-added';
            notices.clear(item);
            await dialog.message(String(error), { title: '无法连接 Anki', kind: 'error' });
        } else if (phase === 'write') {
            if (noteSent && fields != null) {
                await settleFailedWrite(item, error, { deckName, modelName, fields, writeStartedAt }, context);
                return;
            }
            // 写入类失败通常是偶发的，就地提示并给出重试，不打断连续加卡
            item.status = 'not-added';
            notices.clear(item);
            notices.show(item, {
                severity: 'error',
                message: `添加失败：${error instanceof Error ? error.message : String(error)}`,
                actionLabel: '重试',
            }, 8000, retry);
        } else {
            // 准备牌组/模板：错误框由 prepareDeckAndModel 自己弹出
            item.status = 'not-added';
            notices.clear(item);
        }
    }
}

/**
 * 写入请求已发出却报错（超时、连接中断、AnkiConnect 报错）时决定卡片状态。
 *
 * 前端没等到响应不代表 Anki 没写入：先向 Anki 核对一次，确实写入了就按已添加处理（并接上笔记 id，
 * 之后仍能移除）；确认没有才回到未添加并给出重试；连核对都失败时状态未知，回到未添加但不给“重试”，
 * 提示用户先去 Anki 里确认，避免重复添加
 */
async function settleFailedWrite(
    item: ItemModel<unknown>,
    error: unknown,
    write: { deckName: string; modelName: string; fields: ReturnType<typeof anki.makeFields>; writeStartedAt: number; },
    context: CardActionContext,
): Promise<void> {
    const { ankiService, notices, retry } = context;
    notices.show(item, { severity: 'informational', message: '正在核对添加结果…', progress: true }, 0);
    let foundId: number | null = null;
    let reconcileFailed = false;
    try {
        foundId = await ankiService.findAddedMarkerNote(write.deckName, write.modelName, write.fields, write.writeStartedAt);
    } catch (reconcileError) {
        console.error(reconcileError);
        reconcileFailed = true;
    }
    notices.clear(item);
    const message = error instanceof Error ? error.message : String(error);
    if (foundId != null) {
        console.warn(`[anki] addNote 报错，但核对发现笔记已写入（id=${foundId}），按已添加处理`);
        item.id = foundId;
        item.status = 'is-added';
        return;
    }
    item.status = 'not-added';
    if (reconcileFailed) {
        notices.show(item, {
            severity: 'warning',
            message: `无法确认是否已添加（${message}）。请先在 Anki 中查看，避免重复添加。`,
        }, 10000);
        return;
    }
    notices.show(item, { severity: 'error', message: `添加失败：${message}`, actionLabel: '重试' }, 8000, retry);
}

/** 把一张已添加的卡片从 Anki 移除 */
export async function removeCardFromAnki(item: ItemModel<unknown>, context: CardActionContext): Promise<void> {
    const { ankiService, notices, retry } = context;
    item.status = 'processing-remove';
    const operation = createCardOperation(notices, item, '正在从 Anki 移除…');
    try {
        await operation.race(ankiService.deleteNotes([item.id!], operation.signal));
        item.id = null;
        item.status = 'not-added';
        operation.dispose();
        notices.clear(item);
    } catch (error) {
        operation.dispose();
        if (operation.stopped) {
            item.status = 'is-added';
            notices.clear(item);
            notices.show(item, { severity: 'informational', message: '已停止移除' }, 1500);
            return;
        }
        item.status = 'is-added';
        console.error(error);
        notices.clear(item);
        notices.show(item, {
            severity: 'error',
            message: `移除失败：${error instanceof Error ? error.message : String(error)}`,
            actionLabel: '重试',
        }, 8000, retry);
    }
}

export interface UseAddToAnkiOptions {
    wordItems: WordItems;
    selectedDict: Ref<DictName>;
    selectedPronunciation: Ref<'en' | 'us'>;
    /** AI 优选卡片的状态（只在加卡快照时读取） */
    aiPick: aiPickLogic.AiPickState;
    /** 生成卡片例句的 HTML（只在加卡快照时调用） */
    makeSentenceHTML: () => string;
    notices: CardNotices;
    /** 页面初始化后才就绪的配置与 Anki 服务 */
    services: () => { config: cfg.Config; ankiService: anki.AnkiService; };
}

/** 词典卡片与 AI 优选卡片上的加卡、移除、编辑笔记 */
export function useAddToAnki(options: UseAddToAnkiOptions) {
    const { wordItems, selectedDict, selectedPronunciation, aiPick, makeSentenceHTML, notices, services } = options;

    /** 在第一个 await 之前快照一次加卡的全部写入内容（见 AddNoteSnapshot） */
    function takeSnapshot(dictionary: DictName, index: number, item: ItemModel<AddNoteSnapshot['entry']>): AddNoteSnapshot {
        const { config } = services();
        return {
            dictionary,
            entry: item.item,
            sentenceHTML: makeSentenceHTML(),
            aiNote: aiNoteFor(aiPick, dictionary, index),
            pronunciationType: selectedPronunciation.value,
            deckName: cfg.effectiveTextSetting('deckName', config.deckName),
            modelName: cfg.effectiveTextSetting('modelName', config.modelName),
        };
    }

    /**
     * 切换指定词典中指定条目的加卡状态（添加/取消添加）。
     *
     * 词典参数独立出来，是为了让 AI 优选卡片能跨词典操作其选中的条目（可能与当前 tab 不同）；
     * AI 笔记注入判断仍按 (dictionary, index) 比对 AI 优选命中的条目，仅命中条目带笔记时写入“笔记”字段
     */
    async function changeItemAddedOf(dictionary: DictName, index: number) {
        const item = wordItems[dictionary][index];
        const context: CardActionContext = {
            ankiService: services().ankiService,
            notices,
            retry: () => { void changeItemAddedOf(dictionary, index); },
        };
        // 上一次操作留下的提示（含失败提示）在这里清掉，避免与新一次操作叠加
        notices.clear(item);
        if (item.status === 'not-added') { // add to Anki
            await addCardToAnki(item, takeSnapshot(dictionary, index, item), context);
        } else if (item.status === 'is-added') { // remove from Anki
            await removeCardFromAnki(item, context);
        }
    }

    /** 词典卡片的加卡入口：操作当前所选词典中的条目 */
    async function changeItemAdded(index: number) {
        await changeItemAddedOf(selectedDict.value, index);
    }

    /** AI 优选卡片的加卡入口：操作 AI 选中的那条真实词典条目，可能与当前 tab 不同词典 */
    async function handleAiPickAdd() {
        if (aiPick.pick != null) {
            await changeItemAddedOf(aiPick.pick.source, aiPick.pick.index);
        }
    }

    /**
     * 点击单词卡片的“编辑笔记”按钮后，打开 Anki 的编辑对话框。
     *
     * Anki 26.08 重构了编辑窗口（移除了 Ui_Dialog 的 buttonBox），导致 AnkiConnect 的
     * guiEditNote 报错。
     *
     * 因此在 guiEditNote 失败时降级为打开 Anki 卡片浏览器并定位到该笔记，卡片浏览器中可直接编辑。
     */
    async function openEditDialogOf(dictionary: DictName, index: number) {
        const item = wordItems[dictionary][index];
        if (item.id == null) {
            console.error('item.id is null when openEditDialog, item:', item);
            return;
        }
        const { ankiService } = services();
        try {
            await ankiService.guiEditNote(item.id);
        } catch (error) {
            console.error(error);
            try {
                await ankiService.guiBrowse(`nid:${item.id}`);
                // 降级结果就写在触发的这张卡片里；文案较长，多留一点阅读时间
                notices.show(item, {
                    severity: 'warning',
                    message: '编辑对话框打开失败（Anki 26.08 与 AnkiConnect 不兼容），已在 Anki 卡片浏览器中定位该笔记，选中后可在右侧直接编辑。',
                }, 8000);
            } catch (fallbackError) {
                console.error(fallbackError);
                await dialog.message(String(error), { title: '打开编辑对话框失败', kind: 'error' });
            }
        }
    }

    /** 词典卡片的编辑入口：操作当前所选词典中的条目 */
    async function openEditDialog(index: number) {
        await openEditDialogOf(selectedDict.value, index);
    }

    /** AI 优选卡片的编辑入口：操作 AI 选中的那条真实词典条目，可能与当前 tab 不同词典 */
    async function handleAiPickEdit() {
        if (aiPick.pick != null) {
            await openEditDialogOf(aiPick.pick.source, aiPick.pick.index);
        }
    }

    return { changeItemAddedOf, changeItemAdded, handleAiPickAdd, openEditDialogOf, openEditDialog, handleAiPickEdit };
}
