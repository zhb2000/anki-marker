import { ref } from 'vue';
import type { Router } from 'vue-router';

import * as api from '../../tauri-api';
import * as utils from '../../logics/utils';
import { useSettingsStore } from '../../logics/settings-store';
import { dialog } from '../../fluent-controls';
import {
    captureFailureDialog,
    createCaptureDelivery,
    isSameCapturedPayload,
    markCapturedWord,
    planCapturedSentence,
    type CaptureFailure,
    type CapturedSentencePayload,
} from './captureLogic';
import type { SentenceEditor } from './useSentenceEditor';

export interface UseCaptureListenerOptions {
    router: Router;
    editor: Pick<SentenceEditor, 'tokens' | 'showEdit' | 'replaceSentence' | 'changeEditStatus'>;
    /** 以“显式”语义写入搜索框（见 useQueryDispatch） */
    setQueryExplicitly: (text: string) => void;
}

/** 全局快捷键划词录入：监听后端的捕获事件，并取回主窗口重建期间暂存的结果 */
export function useCaptureListener(options: UseCaptureListenerOptions) {
    const { router, editor, setQueryExplicitly } = options;

    /**
     * 后台仍在补取完整句子（Rust 侧重试在途）：句子面板角上的指示环据此显示/收束。
     * 补取结果会覆盖先到的降级结果（仅词），指示环只是让“还会变”这件事有个连续、安静的信号。
     */
    const captureSettling = ref(false);

    /**
     * 用户是否改动过当前内容（编辑句子、输入查询、手动标记）。迟到的后台补取结果据此丢弃——
     * 补取属于同一次划词的“初始状态”，用户已经动手就不该再被覆盖。新的一次捕获会重置它。
     */
    let userTouched = false;

    /** 记一次用户改动（输入查询、编辑句子、标记、粘贴）：迟到的补取结果不再覆盖工作区 */
    function markUserTouched(): void {
        userTouched = true;
    }

    /**
     * 录入捕获的句子：整体替换当前句子（分词同步重建为全新状态）并退出编辑模式。
     * 取句命中的单词作为查询直接写入搜索框，同时在分词结果中尽力预选（位置提示）
     */
    async function applyCapturedSentence(payload: CapturedSentencePayload) {
        const plan = planCapturedSentence(payload, userTouched);
        if (plan.kind === 'empty') {
            return;
        }
        if (plan.kind === 'stale-retry') {
            console.log('[capture] dropping the late retry result: the user has already edited the workspace');
            return;
        }
        // 捕获来源日志（console 会转发到应用日志文件）：定位“错句覆盖”类问题时
        // 区分事件/暂存回放，并与 Rust 侧的 capture 日志对照
        console.log(
            `[capture] applying: word=${JSON.stringify(payload.word)}, retry=${payload.retry}, text=${JSON.stringify(plan.text.slice(0, 80))}`
        );
        // 按快捷键取词时应用可能停留在设置页：切回主界面，避免句子已被替换但用户看到的仍是设置页；
        // 离开前立即落盘设置页可能还未防抖保存的修改（与设置页返回按钮行为一致）
        if (router.currentRoute.value.path !== '/') {
            await useSettingsStore().flush();
            await router.push('/');
        }
        // 先整体替换：分词同步重建为全新状态（每次捕获都是全新状态），进行中的编辑草稿一并作废
        editor.replaceSentence(plan.text);
        if (editor.showEdit.value) {
            await editor.changeEditStatus();
        }
        if (plan.word != null) {
            // 捕获词是查询的权威来源，直接写入搜索框；分词面板的预选只是位置提示，
            // 分词粒度与选区不一致时（如选中的是 encoder-decoder 里的 encoder）
            // 标记会落在整个复合词元上，与查询不等价是允许的
            setQueryExplicitly(plan.word);
            // replaceSentence 已同步重建分词，可直接标记命中的单词
            markCapturedWord(editor.tokens.value, plan.word, plan.wordOffset, plan.text);
        } else {
            // 未命中单词（如手动跨句选择）：句子已整体替换，旧查询一并清空
            setQueryExplicitly('');
        }
        userTouched = false;
    }

    /** 弹出划词未录入内容的提示 */
    function showCaptureFailureDialog(failure: CaptureFailure) {
        const { title, message } = captureFailureDialog(failure, api.os.type());
        void dialog.message(message, { title, kind: 'error' });
    }

    /** 结果已经由事件送达：取走后端的暂存并丢弃（失败只记日志，不影响本次结果的应用） */
    function discardPending(command: 'take_pending_sentence' | 'take_pending_capture_failure'): void {
        utils.invoke<unknown>(command).catch((error: unknown) => {
            console.error(error);
        });
    }

    /** 监听划词句子事件，并取走可能在主窗口重建期间暂存的句子 */
    async function initSentenceCapture() {
        // 后台补取状态：先挂监听、再主动查一次。重试在窗口显示之前就已启动，窗口被重建时
        // “开始”事件必然丢失，只能靠 is_capture_settling 查回；若挂监听后已收到过事件，
        // 则以事件为准（避免被这次查询返回的稍旧值回退）
        let settlingEventReceived = false;
        try {
            await api.event.listen<boolean>('sentence-capture-settling', event => {
                settlingEventReceived = true;
                captureSettling.value = event.payload;
            });
            if (!settlingEventReceived) {
                captureSettling.value = await utils.invoke<boolean>('is_capture_settling');
            }
        } catch (error) {
            console.error(error);
        }
        // 主窗口被关闭后按快捷键会重建窗口，前端就绪之前 emit 的事件会丢失，
        // 此时通过 take_pending_sentence 取回 Rust 侧暂存的句子（两条送达路径的去重见 createCaptureDelivery）
        const sentenceDelivery = createCaptureDelivery<CapturedSentencePayload>({
            apply: applyCapturedSentence,
            discardPending: () => discardPending('take_pending_sentence'),
            isSame: isSameCapturedPayload,
        });
        try {
            await api.event.listen<CapturedSentencePayload>('sentence-captured', event => {
                sentenceDelivery.onEvent(event.payload);
            });
            await sentenceDelivery.onPending(await utils.invoke<CapturedSentencePayload | null>('take_pending_sentence'));
        } catch (error) {
            console.error(error);
        }
        // 划词未录入内容时会弹出主窗口并提示；若前端当时尚未就绪，事件已丢失，
        // 此处取回暂存的失败原因补弹提示
        const failureDelivery = createCaptureDelivery<CaptureFailure>({
            apply: showCaptureFailureDialog,
            discardPending: () => discardPending('take_pending_capture_failure'),
            isSame: (a, b) => a === b,
        });
        try {
            await api.event.listen<CaptureFailure>('sentence-capture-failed', event => {
                failureDelivery.onEvent(event.payload);
            });
            await failureDelivery.onPending(await utils.invoke<CaptureFailure | null>('take_pending_capture_failure'));
        } catch (error) {
            console.error(error);
        }
    }

    return { captureSettling, markUserTouched, initSentenceCapture };
}
