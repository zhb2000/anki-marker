<script setup lang="ts">
import { ref, reactive, watch, nextTick, onBeforeMount, computed } from 'vue';
import { useRouter } from 'vue-router';
import * as api from '../tauri-api';

import * as utils from '../logics/utils';
import * as dict from '../logics/dict';
import * as anki from '../logics/anki';
import * as cfg from '../logics/config';
import * as globals from '../logics/globals';
import * as preference from '../logics/preference';
import { FluentButton, FluentSelect, FluentInput, FluentRadio } from '../fluent-controls';
import {
    CardStatus,
    SentencePanel,
    CollinsCard,
    OxfordCard,
    YoudaoCard,
    PlayAudioButton,
    SettingButton,
    ScrollMemory
} from '../components';


const router = useRouter();
const pageInitialized = ref(false);
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
const selectedDict = ref<'collins' | 'oxford' | 'youdao'>('collins');
/** 搜索框文本 */
const searchText = ref('');
/** 每本字典中的搜索结果 */
const wordItems = reactive({
    'collins': [] as ItemModel<dict.CollinsItem>[],
    'oxford': [] as ItemModel<dict.OxfordItem>[],
    'youdao': [] as ItemModel<dict.YoudaoItem>[]
});
/** 将有道词典的搜索结果分为 concise, web, phrase 三类 */
const wordItemsYoudao = computed(() => {
    const itemModels = {
        'concise': [] as ItemModel<dict.YoudaoItem>[],
        'web': [] as ItemModel<dict.YoudaoItem>[],
        'phrase': [] as ItemModel<dict.YoudaoItem>[]
    };
    for (const wordItem of wordItems.youdao) {
        itemModels[wordItem.item.meaningType].push(wordItem);
    }
    return itemModels;
});
/** 正在搜索或上一次成功搜索的单词，用于防止重复搜索 */
const searchingOrSearchedWords = {
    'collins': '',
    'oxford': '',
    'youdao': ''
};
/** 所选的发音 */
const selectedPronunciation = ref<'en' | 'us'>('us');

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
    await searchAndUpdate(searchText.value, newSelected);
});

/** 搜索框中的文字改变时，搜索新单词 */
watch(searchText, () => {
    if (selectedDict.value === 'youdao') {
        throttledYoudaoSearch();
    } else {
        throttledSearch();
    }
});

/** 保存所选的字典 */
watch(selectedDict, newDict => {
    preference.set('selectedDict', newDict);
});

/** 保存所选的发音 */
watch(selectedPronunciation, newPronunciation => {
    preference.set('selectedPronunciation', newPronunciation);
});

/** 输入框内容改变时调用节流版搜索函数，避免过于频繁地搜索 */
const throttledSearch = utils.throttle(async () => {
    await searchAndUpdate(searchText.value, selectedDict.value);
}, 200);

const throttledYoudaoSearch = utils.throttle(async () => {
    await searchAndUpdate(searchText.value, 'youdao');
}, 400);

/**
 * 搜索单词，并用搜索结果更新 wordItems。
 * 
 * 点击“查询”按钮或按下回车键时直接调用此函数。
 */
async function searchAndUpdate(word: string, dictionary: 'collins' | 'oxford' | 'youdao') {
    word = word.trim();
    if (word.length === 0) {
        wordItems[dictionary] = [];
        // 界面上的单词列表已清空，无法复用，因此对于任何单词都要重新搜索
        searchingOrSearchedWords[dictionary] = '';
        return;
    }
    if (word === searchingOrSearchedWords[dictionary]) {
        // 无需重复搜索
        // - 若 word 正在被搜索，则等待搜索结果即可
        // - 若 word 上次已被搜索成功，则复用界面上的单词列表
        return;
    }
    // 新单词的搜索请求，清空界面上的已有的搜索结果
    wordItems[dictionary] = [];
    // 函数体此前无 await，因此此处更新 searchingOrSearchedWords 遵循 searchAndUpdate 的调用顺序
    // 即，后续的 searchAndUpdate 调用会覆盖前面的 searchingOrSearchedWords
    searchingOrSearchedWords[dictionary] = word;
    let results: dict.CollinsItem[] | dict.OxfordItem[] | dict.YoudaoItem[];
    try {
        if (dictionary === 'collins') {
            results = await dict.searchCollins(word);
        } else if (dictionary === 'oxford') {
            results = await dict.searchOxford(word);
        } else if (dictionary === 'youdao') {
            results = await dict.searchYoudao(word);
        } else {
            throw new Error(`Unknown dictionary: ${String(dictionary)}`);
        }
        // 由于每次搜索的网络延迟不等长，await 后控制流可能晚于后续的 searchAndUpdate调用，那么本次搜索结果将无效
        // 根据 searchingOrSearchedWords[dictionary] 是否被覆盖即可判断本次搜索结果是否仍然有效
    } catch (error) {
        // 仅在本次搜索单词与当前 searchingOrSearchedWords 相同时才显示错误
        // 否则，说明存在新的 searchAndUpdate 调用（它覆盖了 searchingOrSearchedWords），本次搜索结果无效
        if (searchingOrSearchedWords[dictionary] === word) {
            searchingOrSearchedWords[dictionary] = '';
            console.error(error);
            await api.dialog.message(String(error), { title: '查询失败', kind: 'error' });
        }
        return;
    }
    // 仅在本次搜索结果有效时更新搜索结果单词列表
    if (searchingOrSearchedWords[dictionary] === word) {
        wordItems[dictionary] = results.map(item => ({ item, status: 'not-added', id: null }) as ItemModel<any>);
    }
}
// #endregion

// #region 文本框句子编辑
/** 文本框中的句子 */
const sentence = ref('');
/** 是否显示文本框 */
const showEdit = ref(false);
/** 文本框控件 */
const editTextArea = ref<HTMLTextAreaElement | null>(null);
const isMacOS = computed(() => api.os.type() === 'macos');

/** 根据平台显示不同的快捷键提示 */
const editPlaceholder = computed(() => {
    return isMacOS.value
        ? '⌘ + Enter 完成编辑'
        : 'Ctrl + Enter 完成编辑';
});

/** 文本框中的句子被更改时，更新 tokens */
watch(sentence, newSentence => {
    tokens.value = utils.string.tokenize(newSentence).map(token => ({ token, marked: false }));
});

async function pasteToEdit() {
    const text = await api.clipboard.readText();
    if (text != null) {
        sentence.value = text.trim();
        if (showEdit.value) {
            await changeEditStatus();
        }
    }
}

async function changeEditStatus() {
    showEdit.value = !showEdit.value;
    if (showEdit.value) {
        await nextTick();
        editTextArea.value?.focus();
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

    let newModelCreated = false;
    try {
        newModelCreated = (await Promise.all([prepareDeck(deckName), prepareModel(modelName)]))[1];
    } catch (error) {
        await api.dialog.message(String(error), { title: errorTitle!, kind: 'error' });
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

async function changeItemAdded(index: number) {
    const selected = selectedDict.value;
    const item = wordItems[selected][index];
    const pronunciationType = selectedPronunciation.value;
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
            let audioResult = await dict.makePronunciationURL(word, pronunciationType);
            if (audioResult == null) {
                audioResult = {
                    url: dict.makeYoudaoDictVoiceUrl(word, pronunciationType),
                    dict: 'youdao'
                };
            }
            const audioFilename = await dict.makePronunciationFilename(word, pronunciationType, audioResult.dict);
            const id = await ankiService.addMarkerNote(
                config.deckName,
                config.modelName,
                fields,
                audioResult.url,
                audioFilename
            );
            if (id == null) {
                throw new Error('addNote returns null');
            }
            item.id = id;
            item.status = 'is-added';
        } catch (error) {
            item.status = 'not-added';
            console.error(error);
            await api.dialog.message(String(error), { title: '添加失败', kind: 'error' });
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
            await api.dialog.message(String(error), { title: '删除失败', kind: 'error' });
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
        await ankiService.guiEditNote(item.id);
    } catch (error) {
        console.error(error);
        await api.dialog.message(String(error), { title: '打开编辑对话框失败', kind: 'error' });
    }
}

function makeSentenceHTML(): string {
    return tokens.value.map(
        ({ token, marked }) => marked
            ? `<b>${utils.string.escapeHTML(token)}</b>`
            : utils.string.escapeHTML(token)
    ).join('');
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
    // 恢复用户选项
    const cachedDict = preference.get('selectedDict') as 'collins' | 'oxford' | 'youdao';
    if (cachedDict != null && ['collins', 'oxford', 'youdao'].includes(cachedDict)) {
        selectedDict.value = cachedDict;
    }
    const cachedPronunciation = preference.get('selectedPronunciation') as 'en' | 'us';
    if (cachedPronunciation != null && (['en', 'us'] as const).includes(cachedPronunciation)) {
        selectedPronunciation.value = cachedPronunciation;
    }
    pageInitialized.value = true;
    if (!await utils.rustInRelease()) {
        sentence.value = 'The quick brown fox jumps over the lazy dog.'; // test sentence in dev mode
    }
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
                autocomplete="off" @keydown.enter="searchAndUpdate(searchText, selectedDict)" />
            <FluentButton class="header-button" @click="searchAndUpdate(searchText, selectedDict)">查询
            </FluentButton>
            <FluentSelect class="header-select" v-model="selectedDict" name="dict">
                <option value="collins">柯林斯词典</option>
                <option value="oxford">新牛津英汉双解</option>
                <option value="youdao">有道在线词典</option>
            </FluentSelect>
            <SettingButton @click="router.push('/settings')"
                :update-available="globals.appUpdateAvailable.value || globals.templateUpdateAvailable.value" />
        </div>
        <div class="sentence-container">
            <SentencePanel :tokens="tokens" v-if="!showEdit" />
            <textarea class="fluent-textarea" v-model.trim="sentence" v-if="showEdit" ref="editTextArea"
                :placeholder="editPlaceholder" @keydown="handleEditTextAreaKeydown"></textarea>
        </div>
        <div class="words-container">
            <div class="pronunciation-container" v-show="searchText.length > 0">
                <PlayAudioButton :word="searchText" :type="selectedPronunciation" />
                <FluentRadio v-model="selectedPronunciation" value="en" label="英式" name="pronunciation"
                    class="pronunciation-radio-box" />
                <FluentRadio v-model="selectedPronunciation" value="us" label="美式" name="pronunciation"
                    class="pronunciation-radio-box" />
            </div>
            <ScrollMemory :show="selectedDict === 'collins'" class="words-container-inner">
                <CollinsCard v-for="(item, index) in wordItems['collins']" :key="index" :item="item.item" :index="index"
                    :status="item.status" @add-btn-click="changeItemAdded" @edit-btn-click="openEditDialog" />
            </ScrollMemory>
            <ScrollMemory :show="selectedDict === 'oxford'" class="words-container-inner">
                <OxfordCard v-for="(item, index) in wordItems['oxford']" :key="index" :item="item.item" :index="index"
                    :status="item.status" @add-btn-click="changeItemAdded" @edit-btn-click="openEditDialog" />
            </ScrollMemory>
            <ScrollMemory :show="selectedDict === 'youdao'" class="words-container-inner">
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['concise'].length > 0" class="youdao-title">
                    简明释义
                </div>
                <YoudaoCard v-for="(item, index) in wordItemsYoudao['concise']" :key="index" :item="item.item"
                    :index="index" :status="item.status" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['web'].length > 0" class="youdao-title">
                    网络释义
                </div>
                <YoudaoCard v-for="(item, index) in wordItemsYoudao['web']" :key="index" :item="item.item"
                    :index="index" :status="item.status" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" />
                <div v-if="selectedDict === 'youdao' && wordItemsYoudao['phrase'].length > 0" class="youdao-title">
                    短语
                </div>
                <YoudaoCard v-for="(item, index) in wordItemsYoudao['phrase']" :key="index" :item="item.item"
                    :index="index" :status="item.status" @add-btn-click="changeItemAdded"
                    @edit-btn-click="openEditDialog" />
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

.youdao-title {
    margin-bottom: 10px;
    user-select: none;
}
</style>
