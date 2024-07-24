<script setup lang="ts">
import { ref, reactive, watch, nextTick, onMounted, onActivated } from 'vue';
import { useRouter } from 'vue-router';
import { readText as clipboardReadText } from '@tauri-apps/api/clipboard';
import { message as dialogMessage } from '@tauri-apps/api/dialog';

import { tokenize, escapeHTML } from '../logics/stringutils';
import { throttle, tauriInRelease } from '../logics/utils';
import * as dict from '../logics/dict';
import * as anki from '../logics/anki';
import * as cfg from '../logics/config';
import { getConfig } from '../logics/globals';
import { FluentButton, FluentSelect, FluentInput, FluentRadio } from '../fluent-controls';
import type { CardStatus } from '../components/CardStatus';
import SentencePanel from '../components/SentencePanel.vue';
import CollinsCard from '../components/CollinsCard.vue';
import OxfordCard from '../components/OxfordCard.vue';
import PlayAudioButton from '../components/PlayAudioButton.vue';
import SettingButton from '../components/SettingButton.vue';

let config: cfg.Config;

let ankiService: anki.AnkiService;

// #region 单词搜索
/** 单词列表的单词卡片 */
interface ItemModel<T> {
    item: T;
    status: CardStatus;
    id: number | null;
}

/** 划词面板的词元 */
const tokens = ref<{ token: string, marked: boolean; }[]>([]);
/** 所选的字典 */
const selectedDict = ref<'collins' | 'oxford'>('collins');
/** 搜索框文本 */
const searchText = ref('');
/** 每本字典中的搜索结果 */
const wordItems = reactive({
    'collins': [] as ItemModel<dict.CollinsItem>[],
    'oxford': [] as ItemModel<dict.OxfordItem>[]
});

/** tokens 的选中状态改变时，更新 searchText 并搜索新单词 */
watch(tokens, async newTokens => {
    const newSearchText = newTokens
        .filter(token => token.marked)
        .map(token => token.token)
        .join(' ');
    searchText.value = newSearchText;
    await searchAndUpdate(newSearchText, selectedDict.value);
}, { deep: true });

/** 所选的词典改变时，在所选词典中搜索新单词 */
watch(selectedDict, async newSelected => {
    wordItems[newSelected] = [];
    await searchAndUpdate(searchText.value, newSelected);
});

/** 搜索框中的文字改变时，搜索新单词 */
watch(searchText, () => {
    throttledSearch();
});

/** 搜索单词，并用搜索结果更新 wordItems */
async function searchAndUpdate(word: string, dictionary: 'collins' | 'oxford') {
    if (word.length === 0) {
        wordItems[dictionary] = [];
        return;
    }
    let results: dict.CollinsItem[] | dict.OxfordItem;
    try {
        results = (dictionary === 'collins')
            ? await dict.searchCollins(word)
            : await dict.searchOxford(word);
    } catch (error) {
        console.error(error);
        await dialogMessage(String(error), { title: '查询失败', type: 'error' });
        return;
    }
    wordItems[dictionary] = results.map(item => ({ item, status: 'not-added', id: null })) as any[];
}

const throttledSearch = throttle(async () => {
    await searchAndUpdate(searchText.value, selectedDict.value);
}, 200);
// #endregion

// #region 文本框句子编辑
/** 文本框中的句子 */
const sentence = ref('');
/** 是否显示文本框 */
const showEdit = ref(false);
/** 文本框控件 */
const editTextarea = ref<HTMLTextAreaElement | null>(null);

/** 文本框中的句子被更改时，更新 tokens */
watch(sentence, newSentence => {
    tokens.value = tokenize(newSentence).map(token => ({ token, marked: false }));
});

async function pasteToEdit() {
    const text = await clipboardReadText();
    if (text != null) {
        sentence.value = text.trim();
        if (showEdit.value) {
            changeEditStatus();
        }
    }
}

async function changeEditStatus() {
    showEdit.value = !showEdit.value;
    if (showEdit.value) {
        await nextTick();
        editTextarea.value?.focus();
    }
}
// #endregion

// #region 单词列表滚动位置的保存与恢复
const router = useRouter();
const wordContainer = ref<HTMLDivElement | null>(null);
let wordContainerScrollTop = 0;

router.beforeEach((_to, from) => {
    if (from.name === 'Main') {
        wordContainerScrollTop = wordContainer.value?.scrollTop ?? 0;
    }
});

onActivated(() => {
    if (router.currentRoute.value.name === 'Main' && wordContainer.value != null) {
        wordContainer.value.scrollTop = wordContainerScrollTop;
    }
});
// #endregion

// #region Anki
/** 若牌组或笔记模板不存在，则创建之 */
async function prepareDeckAndModel(deckName: string, modelName: string) {
    let errorTitle: string | null = null;

    async function prepareDeck(deckName: string) {
        try {
            await ankiService.createDeck(deckName);
        } catch (error) {
            errorTitle = `牌组 ${deckName} 创建失败`;
            throw error;
        }
    }

    async function prepareModel(modelName: string) {
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
                await anki.createMarkerModel(ankiService, modelName);
            } catch (error) {
                errorTitle = `笔记模板 ${modelName} 创建失败`;
                throw error;
            }
        }
    }

    try {
        await Promise.all([prepareDeck(deckName), prepareModel(modelName)]);
    } catch (error) {
        await dialogMessage(String(error), { title: errorTitle!, type: 'error' });
        throw error;
    }
}

async function changeItemAdded(index: number) {
    const selected = selectedDict.value;
    const item = wordItems[selected][index];
    if (item.status === 'not-added') { // add to Anki
        item.status = 'processing-add';
        try {
            await prepareDeckAndModel(config.deckName, config.modelName);
        } catch (error) {
            item.status = 'not-added';
            console.error(error);
            return; // prepareDeckAndModel has already shown the error message
        }
        try {
            const fields = anki.makeFields(selected, item.item, makeSentenceHTML());
            const word = ('phrase' in item.item && item.item.phrase != null)
                ? item.item.phrase
                : item.item.word;
            const id = await ankiService.addNote(
                config.deckName,
                config.modelName,
                fields,
                makePronunciationURL(word),
                makePronunciationFilename(word)
            );
            if (id == null) {
                throw new Error('addNote returns null');
            }
            item.id = id;
            item.status = 'is-added';
        } catch (error) {
            item.status = 'not-added';
            console.error(error);
            await dialogMessage(String(error), { title: '添加失败', type: 'error' });
        }
    } else if (item.status === 'is-added') { // remove from Anki
        item.status = 'processing-remove';
        try {
            await ankiService.deleteNotes([item.id!]);
            item.id = null;
            item.status = 'not-added';
        } catch (error) {
            item.status = 'is-added';
            console.error(error);
            await dialogMessage(String(error), { title: '删除失败', type: 'error' });
        }
    }
}

/** 点击单词卡片的“编辑笔记”按钮后，打开 Anki 的编辑对话框 */
async function openEditDialog(index: number) {
    const selected = selectedDict.value;
    const item = wordItems[selected][index];
    if (item.id == null) {
        console.error('item.id is null when openEditDialog, item:', item);
        return;
    }
    try {
        await ankiService.guiEditNote(item.id!);
    } catch (error) {
        console.error(error);
        await dialogMessage(String(error), { title: '打开编辑对话框失败', type: 'error' });
    }
}

function makeSentenceHTML(): string {
    return tokens.value.map(({ token, marked }) => marked ? `<b>${escapeHTML(token)}</b>` : token).join('');
}
// #endregion

// #region 发音
/** 所选的发音 */
const selectedPronunciation = ref<'en' | 'us'>('us');
const audio = new Audio();

function makePronunciationURL(word: string): string {
    const type = (selectedPronunciation.value === 'en') ? 1 : 2;
    return `https://dict.youdao.com/dictvoice?type=${type}&audio=${encodeURIComponent(word)}`;
}

function makePronunciationFilename(word: string): string {
    return `youdao_${word}_${selectedPronunciation.value}.mp3`;
}

function playPronunciation() {
    audio.pause();
    audio.src = makePronunciationURL(searchText.value);
    audio.play();
}
// #endregion

onMounted(async () => {
    if (tauriInRelease()) {
        document.addEventListener('contextmenu', e => {
            if (!((e.target instanceof HTMLInputElement && e.target.type === 'text')
                || e.target instanceof HTMLTextAreaElement)) {
                e.preventDefault();
            }
        }); // disable context menu in release mode
    } else {
        sentence.value = 'I\'m a student. This is test sentence 123.'; // test sentence in dev mode
    }

    try {
        config = await getConfig();
    } catch (error) {
        console.error(error);
        await dialogMessage(String(error), { title: '配置文件读取失败', type: 'error' });
        return;
    }
    try {
        await cfg.startConfigWatcher(config);
    } catch (error) {
        console.error(error);
        await dialogMessage(String(error), { title: '配置文件监听失败', type: 'error' });
    }

    ankiService = new anki.AnkiService(config.ankiConnectURL);

    /** 监听 config 的 anki-connect-url 更新 */
    watch(() => config.ankiConnectURL, newURL => {
        ankiService.url = newURL;
    });
});
</script>

<template>
    <div class="main-window">
        <div class="header-container">
            <FluentButton class="header-button" :accent="showEdit" @click="changeEditStatus">
                {{ showEdit ? '完成' : '编辑' }}
            </FluentButton>
            <FluentButton class="header-button" @click="pasteToEdit">粘贴</FluentButton>
            <FluentSelect class="header-select" v-model="selectedDict">
                <option value="collins">柯林斯词典</option>
                <option value="oxford">新牛津英汉双解</option>
            </FluentSelect>
            <FluentInput class="header-input-text" type="text" v-model.trim="searchText" placeholder="回车查询单词"
                @keydown.enter="searchAndUpdate(searchText, selectedDict)" />
            <FluentButton class="header-button" @click="searchAndUpdate(searchText, selectedDict)">查询</FluentButton>
            <SettingButton @click="router.push('/settings')" />
        </div>
        <div class="sentence-container">
            <SentencePanel :tokens="tokens" v-if="!showEdit" />
            <textarea class="fluent-textarea" v-model.trim="sentence" v-if="showEdit" ref="editTextarea"
                placeholder="Ctrl + Enter 完成编辑" @keydown.ctrl.enter="changeEditStatus"></textarea>
        </div>
        <div class="words-container">
            <div class="pronunciation-container" v-show="searchText.length > 0">
                <PlayAudioButton @click="playPronunciation" />
                <FluentRadio v-model="selectedPronunciation" value="en" label="英式" name="pronunciation"
                    class="pronunciation-radio-box" />
                <FluentRadio v-model="selectedPronunciation" value="us" label="美式" name="pronunciation"
                    class="pronunciation-radio-box" />
            </div>
            <div ref="wordContainer" class="words-container-inner" v-if="selectedDict === 'collins'">
                <CollinsCard v-for="(item, index) in wordItems['collins']" :item="item.item" :index="index"
                    :status="item.status" @add-btn-click="changeItemAdded" @edit-btn-click="openEditDialog" />
            </div>
            <div class="words-container-inner" v-else>
                <OxfordCard v-for="(item, index) in wordItems['oxford']" :item="item.item" :index="index"
                    :status="item.status" @add-btn-click="changeItemAdded" @edit-btn-click="openEditDialog" />
            </div>
        </div>
    </div>
</template>

<style scoped>
.main-window {
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: calc(34px + 12px * 2) 1fr;
    background-color: var(--window-background);
    height: 100vh;
    grid-template-areas:
        'header header'
        'sentence words';
    overflow-x: hidden;
}

.header-container {
    grid-area: header;
    display: flex;
    justify-content: center;
    align-items: center;
    padding: 12px 15px;
    gap: 5px;
}

.sentence-container {
    grid-area: sentence;
    padding: 15px;
    padding-top: 0;
    padding-right: calc(15px / 2);
    /* 需要设置此属性才能让 SentencePanel 的 overflow-y: auto 生效 */
    overflow-y: hidden;
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

.fluent-textarea {
    resize: none;
    overflow-wrap: break-word;
    width: 100%;
    height: 100%;
    padding: 8px 12px;
    font-family: var(--font-family);
    font-size: var(--font-size);
    background-color: var(--control-background);
    outline: none;
    border-style: solid;
    border-color: var(--border-color);
    border-bottom-color: var(--border-bottom-color);
    border-width: var(--border-width);
    border-radius: var(--border-radius);
}

.fluent-textarea:hover {
    background-color: var(--control-background-hover);
}

.fluent-textarea:focus {
    background-color: var(--input-text-background-focus);
    border-bottom-width: var(--input-text-border-bottom-width-focus);
    border-bottom-color: var(--accent);
}
</style>
