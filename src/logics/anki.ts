import { fetch, Body, ResponseType } from '@tauri-apps/api/http';

import { typeAssertion } from './typing';
import type { CollinsItem, OxfordItem } from './dict';
import { escapeHTML } from './stringutils';

/**
 * Anki Connect API: https://foosoft.net/projects/anki-connect/
 */
export class AnkiService {
    public url: string;

    public constructor(url: string) {
        this.url = url;
    }

    public async invoke<T>(action: string, params?: Record<string, any>): Promise<T> {
        // 使用 Tauri 发送 HTTP 请求，以规避跨域问题
        const responseObj = await fetch(this.url, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: Body.json({ action, params, version: 6 }),
            responseType: ResponseType.JSON
        });
        if (!responseObj.ok) {
            throw new Error(
                'HTTP request is not ok. ' +
                `status: ${responseObj.status}, ` +
                `data: ${JSON.stringify(responseObj.data)}, ` +
                `headers: ${JSON.stringify(responseObj.headers)}.`
            );
        }
        if (!(responseObj.data instanceof Object)) {
            throw TypeError(`Expect response data to be object but receive ${typeof responseObj.data}.`);
        }
        const response = responseObj.data;
        if (!('error' in response)) {
            throw new Error('response is missing required error field');
        }
        if (!('result' in response)) {
            throw new Error('response is missing required result field');
        }
        typeAssertion<{ error: string | null, result: T; }>(response);
        if (response.error != null) {
            throw new Error(response.error);
        }
        return response.result;
    }

    // #region Deck Actions, https://foosoft.net/projects/anki-connect/#deck-actions
    /** Gets the complete list of deck names for the current user. */
    public async deckNames(): Promise<string[]> {
        return await this.invoke('deckNames');
    }

    /** Gets the complete list of deck names and their respective IDs for the current user. */
    public async deckNamesAndIds(): Promise<Record<string, number>> {
        return await this.invoke('deckNamesAndIds');
    }

    /** Create a new empty deck. Will not overwrite a deck that exists with the same name. */
    public async createDeck(deck: string): Promise<number> {
        return await this.invoke('createDeck', { deck });
    }
    // #endregion

    // #region Model Actions, https://foosoft.net/projects/anki-connect/#model-actions
    /** Gets the complete list of model names for the current user. */
    public async modelNames(): Promise<string[]> {
        return await this.invoke('modelNames');
    }

    /** Gets a list of models for the provided model names from the current user. */
    public async findModelsByName(modelNames: string[]): Promise<Record<string, any>[]> {
        return await this.invoke('findModelsByName', { modelNames });
    }

    /**
     * Creates a new model to be used in Anki. User must provide the `modelName`, `inOrderFields` and `cardTemplates`
     * to be used in the model. There are optional fields `css` and `isCloze`. If not specified, `css` will use the
     * default Anki css and `isCloze` will be equal to `False`. If `isCloze` is `True` then model will be created
     * as `Cloze`.
     * 
     * Optionally the `Name` field can be provided for each entry of `cardTemplates`. By default the card names will
     * be `Card 1`, `Card 2`, and so on.
     */
    public async createModel(
        modelName: string,
        inOrderFields: string[],
        cardTemplates: Record<string, string>[],
        css?: string,
        isCloze?: boolean
    ): Promise<number> {
        return await this.invoke('createModel', {
            modelName,
            inOrderFields,
            cardTemplates,
            css,
            isCloze
        });
    }
    // #endregion

    // #region Note Actions, https://foosoft.net/projects/anki-connect/#note-actions
    /**
     * Creates a note using the given deck and model, with the provided field values and tags.
     * Returns the identifier of the created note created on success, and null on failure.
     */
    public async addNote(
        deckName: string,
        modelName: string,
        fields: Fields,
        audioURL: string,
        audioFilename: string
    ): Promise<number | null> {
        return await this.invoke('addNote', {
            note: {
                deckName,
                modelName,
                fields: { ...fields },
                audio: [{
                    url: audioURL,
                    filename: audioFilename,
                    fields: ['发音']
                }],
                options: { allowDuplicate: true }
            }
        });
    }

    /**
     * Deletes notes with the given ids. If a note has several cards associated with it,
     * all associated cards will be deleted.
     */
    public async deleteNotes(notes: number[]) {
        await this.invoke('deleteNotes', { notes });
    }
    // #endregion
}

interface Fields {
    '单词': string;
    '音标'?: string;
    '释义'?: string;
    '笔记'?: string;
    '例句': string;
    'url'?: string;
}

function makeMeaning(item: CollinsItem | OxfordItem): string | null {
    const meaning = [];
    let firstLine: string | null = `<i>${escapeHTML(item.sense ?? '')}</i> ${escapeHTML((item as { ext?: string; }).ext ?? '')}`.trim();
    if (firstLine === '<i></i>') {
        firstLine = null;
    } else {
        meaning.push(firstLine);
    }
    if (item.enDef != null) {
        if (firstLine != null) {
            meaning[0] = `${firstLine}<br>`;
        }
        // Collins 词典的 EnDef 含有 <b></b> 标签，不转义
        meaning.push(`<span class="endef">${item.enDef}</span>`);
    }
    if (item.cnDef != null) {
        if (item.enDef == null && firstLine != null) {
            meaning[0] = `${firstLine}<br>`;
        }
        meaning.push(`<span class="cndef">${escapeHTML(item.cnDef)}</span>`);
    }
    return (meaning.length > 0) ? meaning.join('<br>') : null;
}

export function makeFields(
    dict: 'collins' | 'oxford',
    item: CollinsItem | OxfordItem,
    sentence: string
): Fields {
    if (dict === 'collins') {
        typeAssertion<CollinsItem>(item);
        return {
            '单词': item.word,
            '音标': item.phonetic ?? undefined,
            '释义': makeMeaning(item) ?? undefined,
            '例句': sentence
        };
    } else {
        typeAssertion<OxfordItem>(item);
        return {
            '单词': item.phrase ?? item.word,
            '音标': item.phonetic ?? undefined,
            '释义': makeMeaning(item) ?? undefined,
            '例句': sentence
        };
    }
}

import MODEL_FRONT from '../assets/model-front.html?raw';
import MODEL_BACK from '../assets/model-back.html?raw';
import MODEL_CSS from '../assets/model-css.html?raw';

export async function createMarkerModel(service: AnkiService, modelName: string) {
    await service.createModel(
        modelName,
        ["单词", "音标", "释义", "笔记", "例句", "url", "发音"],
        [{
            "Name": "card1",
            "Front": MODEL_FRONT,
            "Back": MODEL_BACK
        }],
        MODEL_CSS
    );
}
