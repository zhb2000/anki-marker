<script setup lang="ts">
import { ref, watch, onBeforeMount } from 'vue';
import { useRouter } from 'vue-router';

import * as utils from '../logics/utils';
import * as anki from '../logics/anki';
import * as cfg from '../logics/config';
import * as globals from '../logics/globals';
import AiPickCard from '../components/AiPickCard.vue';
import {
    FluentButton, FluentSelect, FluentInput, FluentTextarea, FluentRadio, FluentStatusText,
} from '../fluent-controls';
import {
    SentencePanel,
    SentenceCaptureRing,
    CollinsCard,
    OxfordCard,
    YoudaoCard,
    PlayAudioButton,
    SettingButton,
    ScrollMemory
} from '../components';
import { createCardNotices } from './main/cardNotices';
import { useWordSearch } from './main/useWordSearch';
import { useSentenceEditor } from './main/useSentenceEditor';
import { useAiPick } from './main/useAiPick';
import { useQueryDispatch } from './main/useQueryDispatch';
import { useCaptureListener } from './main/useCaptureListener';
import { useAddToAnki } from './main/useAddToAnki';
import type { DictName } from './main/types';

// 主界面按功能区拆成若干 composable（见 ./main），这里只做组装与跨区的事件处理。
// 区与区之间的依赖一律经参数/返回值显式传递；页面初始化后才就绪的 config 与 ankiService 以 getter 传入

const router = useRouter();
const pageInitialized = ref(false);
let config: cfg.Config;
let ankiService: anki.AnkiService;

/** 词典卡片的就地反馈（加卡进度、失败与重试、编辑降级） */
const notices = createCardNotices();

// #region 单词搜索
const search = useWordSearch();
const {
    searchText, selectedDict, selectedPronunciation, wordItems, wordItemsYoudao,
    pronunciationNotice, handlePronunciationError, searchNotice,
} = search;
// #endregion

// #region 文本框句子编辑
const editor = useSentenceEditor({
    onSentenceEdited: () => {
        // 句子改过 = 语境变了：按同一个查询词重新取释义。查询由用户书写，编辑句子不改写它，
        // 但 AI 优选的缓存 key 含句子，会据此重新请求（同句同词命中结果缓存，不重复拉取）。
        // 没改动过则什么都不做，避免白白消耗一次 LLM 请求
        if (searchText.value.trim().length > 0) {
            void query.submitSearch();
        }
    },
});
const {
    tokens, sentence, showEdit, editPlaceholder, pasteNotice,
    changeEditStatus, handleEditTextAreaKeydown,
} = editor;
// #endregion

// #region AI 优选释义
const ai = useAiPick({
    sentence,
    searchText,
    wordItems,
    searchAndUpdate: search.searchAndUpdate,
    getConfig: () => config,
});
const { aiPick, showAiPickCard, aiPickCardStatus, aiPickNotice, aiPickItem, isAiPicked, stopAiPick, regenerateAiPick } = ai;
// #endregion

// #region 查询调度
const query = useQueryDispatch({ search, ai });
const { submitSearch } = query;
// #endregion

// #region 全局快捷键划词录入
const capture = useCaptureListener({ router, editor, setQueryExplicitly: query.setQueryExplicitly });
const { captureSettling } = capture;
// #endregion

// #region Anki
const cards = useAddToAnki({
    wordItems,
    selectedDict,
    selectedPronunciation,
    aiPick,
    makeSentenceHTML: editor.makeSentenceHTML,
    notices,
    services: () => ({ config, ankiService }),
});
const { changeItemAdded, openEditDialog, handleAiPickAdd, handleAiPickEdit } = cards;
// #endregion

// #region 跨区的用户操作
/** 记一次用户改动（输入查询）：迟到的补取结果不再覆盖工作区 */
function onUserEdit(): void {
    capture.markUserTouched();
}

/** 编辑句子框的输入：既算用户改动，也标记本次编辑会话确实改过句子 */
function onSentenceEdit(): void {
    capture.markUserTouched();
    editor.markSentenceDirty();
}

/**
 * 用户标记手势结束（单击松手、拖刷松手、Shift 区间松开）：把当前标记词元按单空格拼接后
 * 写入搜索框，等价于用户手打这段文本。这是有损的快捷输入——只送单词词元，丢原始标点与空白。
 */
function onMarksCommitted(): void {
    capture.markUserTouched();
    query.setQueryExplicitly(tokens.value
        .filter(token => token.marked)
        .map(token => token.token)
        .join(' '));
}

/** SentencePanel 上报的标记写入：tokens 的写入收敛于此（整体替换与编辑重建除外）。标记不触发查询，查询只在手势结束时快送一次 */
function onMarkToken(index: number, value: boolean): void {
    capture.markUserTouched();
    tokens.value[index].marked = value;
}

async function pasteToEdit() {
    const text = await editor.readPastedSentence();
    if (text == null) {
        return;
    }
    // 粘贴属于整体替换句子：分词同步重建为全新状态，不保留旧的标记，编辑草稿一并作废；
    // 旧句子的查询随之作废（查询不由标记派生，需显式清空）
    editor.replaceSentence(text);
    query.setQueryExplicitly('');
    capture.markUserTouched();
    if (showEdit.value) {
        await changeEditStatus();
    }
}

/** 点击卡片提示上的操作（停止 / 重试） */
function handleCardNoticeAction(dictionary: DictName, index: number): void {
    notices.runAction(wordItems[dictionary][index]);
}

/** AI 优选卡片上提示的操作（停止 / 重试）：转发给源条目 */
function handleAiPickNoticeAction(): void {
    if (aiPick.pick != null) {
        handleCardNoticeAction(aiPick.pick.source, aiPick.pick.index);
    }
}
// #endregion

// 由于使用了 KeepAlive 不销毁页面，所以只会执行一次
onBeforeMount(async () => {
    // 为需要初始化的变量赋值
    await globals.initAtAppStart();
    [config, ankiService] = await Promise.all([
        globals.getConfig(),
        globals.getAnkiService()
    ]);
    // 关闭 AI 优选功能时：中止进行中的请求并重置卡片，界面恢复原样（与停止按钮同一动作）
    watch(() => config.llmEnabled, enabled => {
        if (!enabled) {
            stopAiPick();
        }
    });
    // 恢复用户选项
    search.restorePreferences();
    pageInitialized.value = true;
    if (!await utils.rustInRelease()) {
        editor.replaceSentence('The quick brown fox jumps over the lazy dog.'); // test sentence in dev mode
    }
    // 放在 dev 测试句子之后，捕获的句子可覆盖测试句子
    await capture.initSentenceCapture();
});
</script>

<template>
    <div v-if="pageInitialized" class="main-window">
        <div class="header-container">
            <FluentButton class="header-button" :accent="showEdit" @click="changeEditStatus">
                {{ showEdit ? '完成' : '编辑' }}
            </FluentButton>
            <FluentButton class="header-button" @click="pasteToEdit">粘贴</FluentButton>
            <FluentInput class="header-input-text" type="text" v-model="searchText" placeholder="回车查询单词" name="search"
                @input="onUserEdit"
                autocomplete="off" @keydown.enter="submitSearch()" />
            <FluentButton class="header-button" @click="submitSearch()">查询
            </FluentButton>
            <FluentSelect class="header-select" v-model="selectedDict" name="dict">
                <option value="collins">柯林斯词典</option>
                <option value="oxford">新牛津英汉双解</option>
                <option value="youdao">有道在线词典</option>
            </FluentSelect>
            <SettingButton @click="router.push('/settings')" />
        </div>
        <div class="sentence-container">
            <!-- 粘贴失败的就地提示：紧挨着上方的“粘贴”按钮，收起时不占高度 -->
            <FluentStatusText class="sentence-notice" :severity="pasteNotice?.severity ?? 'error'"
                :message="pasteNotice?.message ?? null" />
            <SentencePanel class="sentence-main" :tokens="tokens" v-if="!showEdit" @mark="onMarkToken"
                @commit="onMarksCommitted" />
            <FluentTextarea class="sentence-editor sentence-main" v-model="sentence" v-if="showEdit" ref="editTextArea"
                :placeholder="editPlaceholder" @input="onSentenceEdit" @keydown="handleEditTextAreaKeydown" />
            <SentenceCaptureRing :active="captureSettling" v-if="!showEdit" />
        </div>
        <div class="words-container">
            <div class="pronunciation-container" v-show="searchText.length > 0">
                <PlayAudioButton :word="searchText" :type="selectedPronunciation"
                    @error="handlePronunciationError" />
                <FluentRadio v-model="selectedPronunciation" value="en" label="英式" name="pronunciation"
                    class="pronunciation-radio-box" />
                <FluentRadio v-model="selectedPronunciation" value="us" label="美式" name="pronunciation"
                    class="pronunciation-radio-box" />
                <!-- 发音失败就地提示：追加在行末，只做淡入淡出，不挤动左边的按钮与单选项 -->
                <FluentStatusText class="pronunciation-notice" :collapse="false"
                    :severity="pronunciationNotice?.severity ?? 'error'"
                    :message="pronunciationNotice?.message ?? null" />
            </div>
            <!-- 手打查询失败的就地提示：位于结果列表上方，收起时不占高度 -->
            <FluentStatusText class="search-notice" :severity="searchNotice?.severity ?? 'error'"
                :message="searchNotice?.message ?? null" />
            <ScrollMemory :show="selectedDict === 'collins'" class="words-container-inner">
                <!-- AI 优选卡随列表内容滚动，每个词典容器内各放一份（内容同源）；active 标记当前可见实例，仅它播放动画 -->
                <AiPickCard v-if="showAiPickCard" :state="aiPick" :status="aiPickCardStatus"
                    :picked-item="aiPickItem" :notice="aiPickNotice" :active="selectedDict === 'collins'"
                    @add-btn-click="handleAiPickAdd" @edit-btn-click="handleAiPickEdit" @stop="stopAiPick"
                    @regenerate="regenerateAiPick" @notice-action="handleAiPickNoticeAction" />
                <CollinsCard v-for="(item, index) in wordItems['collins']" :key="index" :item="item.item" :index="index"
                    :status="item.status" :notice="item.notice" :ai-picked="isAiPicked('collins', index)"
                    @add-btn-click="changeItemAdded" @edit-btn-click="openEditDialog"
                    @notice-action="handleCardNoticeAction('collins', $event)" />
            </ScrollMemory>
            <ScrollMemory :show="selectedDict === 'oxford'" class="words-container-inner">
                <AiPickCard v-if="showAiPickCard" :state="aiPick" :status="aiPickCardStatus"
                    :picked-item="aiPickItem" :notice="aiPickNotice" :active="selectedDict === 'oxford'"
                    @add-btn-click="handleAiPickAdd" @edit-btn-click="handleAiPickEdit" @stop="stopAiPick"
                    @regenerate="regenerateAiPick" @notice-action="handleAiPickNoticeAction" />
                <OxfordCard v-for="(item, index) in wordItems['oxford']" :key="index" :item="item.item" :index="index"
                    :status="item.status" :notice="item.notice" :ai-picked="isAiPicked('oxford', index)"
                    @add-btn-click="changeItemAdded" @edit-btn-click="openEditDialog"
                    @notice-action="handleCardNoticeAction('oxford', $event)" />
            </ScrollMemory>
            <ScrollMemory :show="selectedDict === 'youdao'" class="words-container-inner">
                <AiPickCard v-if="showAiPickCard" :state="aiPick" :status="aiPickCardStatus"
                    :picked-item="aiPickItem" :notice="aiPickNotice" :active="selectedDict === 'youdao'"
                    @add-btn-click="handleAiPickAdd" @edit-btn-click="handleAiPickEdit" @stop="stopAiPick"
                    @regenerate="regenerateAiPick" @notice-action="handleAiPickNoticeAction" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['concise'].length > 0" class="youdao-title">
                    简明释义
                </div>
                <YoudaoCard v-for="entry in wordItemsYoudao['concise']" :key="entry.flatIndex" :item="entry.model.item"
                    :index="entry.flatIndex" :status="entry.model.status" :notice="entry.model.notice"
                    :ai-picked="isAiPicked('youdao', entry.flatIndex)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" @notice-action="handleCardNoticeAction('youdao', $event)" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['web'].length > 0" class="youdao-title">
                    网络释义
                </div>
                <YoudaoCard v-for="entry in wordItemsYoudao['web']" :key="entry.flatIndex" :item="entry.model.item"
                    :index="entry.flatIndex" :status="entry.model.status" :notice="entry.model.notice"
                    :ai-picked="isAiPicked('youdao', entry.flatIndex)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" @notice-action="handleCardNoticeAction('youdao', $event)" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['phrase'].length > 0" class="youdao-title">
                    短语
                </div>
                <YoudaoCard v-for="entry in wordItemsYoudao['phrase']" :key="entry.flatIndex" :item="entry.model.item"
                    :index="entry.flatIndex" :status="entry.model.status" :notice="entry.model.notice"
                    :ai-picked="isAiPicked('youdao', entry.flatIndex)" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" @notice-action="handleCardNoticeAction('youdao', $event)" />
            </ScrollMemory>
        </div>
    </div>
    <div v-else class="loading-screen"></div>
</template>

<style scoped>
.loading-screen {
    height: 100vh;
    background-color: var(--window-background);
}

.main-window {
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: calc(34px + 12px * 2) 1fr;
    background-color: var(--window-background);
    height: 100vh;
    /* macOS 覆盖式标题栏：顶部让出原生标题栏高度，内容不会被红绿灯压住 */
    padding-top: var(--titlebar-inset);
    grid-template-areas:
        'header header'
        'sentence words';
    overflow-x: hidden;
    user-select: none;
}

.header-container {
    grid-area: header;
    display: flex;
    justify-content: center;
    align-items: center;
    padding: 12px 15px;
    gap: 5px;
}

/* 发音行末的就地提示：与单选项留出间距；横排内不参与高度联动，也不需要顶部间距 */
.pronunciation-notice {
    --status-gap: 0px;
    margin-left: 10px;
}

.sentence-container {
    grid-area: sentence;
    padding: 15px;
    padding-top: 0;
    padding-right: calc(15px / 2);
    /* 需要设置此属性才能让 SentencePanel 的 overflow-y: auto 生效 */
    overflow-y: hidden;
    /* SentenceCaptureRing 以此为定位基准，钉在句子面板右下角 */
    position: relative;
    /* 纵向排布：顶部可能有一行粘贴失败提示，句子面板（或编辑框）占满其余高度 */
    display: flex;
    flex-direction: column;
}

/* 句子面板与编辑框：占满提示行之外的高度（flex-basis 0 让自身的 height: 100% 不再生效） */
.sentence-main {
    flex: 1 1 0;
    min-height: 0;
}

/* 句子区与结果区上方的就地提示：间距放在提示下方，并留在被折叠的区域内，收起时不留空档 */
.sentence-notice,
.search-notice {
    --status-gap: 0px;
    flex-shrink: 0;
}

.sentence-notice :deep(.status-text),
.search-notice :deep(.status-text) {
    padding-bottom: 8px;
}

/* 与结果列表左右内边距对齐 */
.search-notice {
    padding-left: calc(15px / 2);
    padding-right: 15px;
}

.words-container {
    grid-area: words;
    overflow-y: hidden;
    display: flex;
    flex-direction: column;
}

.words-container-inner {
    flex-grow: 1;
    overflow-y: auto;
    padding: 15px;
    padding-top: 0;
    padding-left: calc(15px / 2);
}

.pronunciation-container {
    padding-top: 0;
    padding-left: calc(15px / 2);
    padding-bottom: 8px;
    display: flex;
    align-items: center;
}

.pronunciation-radio-box {
    margin-right: 8px;
}

.header-button {
    height: 34px;
    padding-left: 12px;
    padding-right: 12px;
}

.header-select {
    height: 34px;
}

.header-input-text {
    height: 34px;
    flex-grow: 1;
}

.sentence-editor {
    width: 100%;
    height: 100%;
}

.youdao-title {
    margin-bottom: 10px;
    user-select: none;
    cursor: default;
}
</style>
