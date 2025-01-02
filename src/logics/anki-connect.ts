import { fetch, Body, ResponseType } from '@tauri-apps/api/http';

import { typeAssertion } from './typing';

/**
 * Anki Connect API: https://foosoft.net/projects/anki-connect/
 */
export class AnkiConnectApi {
    public url: string;

    public constructor(url: string) {
        this.url = url;
    }

    public async invoke<T>(action: string, params?: Record<string, any>): Promise<T> {
        // 使用 Tauri 的 fetch 发送 HTTP 请求，以规避跨域问题
        const response = await fetch(this.url, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: Body.json({ action, params, version: 6 }),
            responseType: ResponseType.JSON
        });
        if (!response.ok) {
            throw new Error(
                'HTTP request is not ok. ' +
                `status: ${response.status}, ` +
                `data: ${JSON.stringify(response.data)}, ` +
                `headers: ${JSON.stringify(response.headers)}.`
            );
        }
        if (!(response.data instanceof Object)) {
            throw TypeError(`Expect response data to be Object but receive ${typeof response.data}: ${response.data}`);
        }
        const data = response.data;
        if (!('error' in data)) {
            throw new Error('response is missing required error field');
        }
        if (!('result' in data)) {
            throw new Error('response is missing required result field');
        }
        typeAssertion<{ error: string | null, result: T; }>(data);
        if (data.error != null) {
            throw new Error(data.error);
        }
        return data.result;
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

    /**
     * Modify the templates of an existing model by name.
     * Only specifies cards and specified sides will be modified.
     * If an existing card or side is not included in the request, it will be left unchanged.
     */
    public async updateModelTemplates(name: string, templates: Record<string, Record<string, any>>) {
        await this.invoke('updateModelTemplates', { model: { name, templates } });
    }

    /** Modify the CSS styling of an existing model by name. */
    public async updateModelStyling(name: string, css: string) {
        await this.invoke('updateModelStyling', { model: { name, css } });
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
        fields: Record<string, string>,
        audio?: { url: string, filename: string, fields: string[]; }[],
        options?: { allowDuplicate: boolean; }
    ): Promise<number | null> {
        return await this.invoke('addNote', {
            note: {
                deckName,
                modelName,
                fields,
                audio,
                options
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

    // #region Graphical Actions, https://foosoft.net/projects/anki-connect/#graphical-actions
    /**
     * Opens the Edit dialog with a note corresponding to given note ID. The dialog is similar to
     * the Edit Current dialog, but:
     * 
     * - has a Preview button to preview the cards for the note
     * - has a Browse button to open the browser with these cards
     * - has Previous/Back buttons to navigate the history of the dialog
     * - has no bar with the Close button
     */
    public async guiEditNote(note: number) {
        await this.invoke('guiEditNote', { note });
    }
    // #endregion
}
