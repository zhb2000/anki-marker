import { describe, expect, it } from 'vitest';

import { classifySentenceChange, remapTokenKeys } from './sentenceChange';

describe('classifySentenceChange', () => {
    const base = 'The model is fast.';

    it('尾部追加 = 并入下文', () => {
        expect(classifySentenceChange(base, base + ' It runs fast.')).toEqual({ kind: 'append', added: 14 });
    });

    it('头部前插 = 并入上文', () => {
        expect(classifySentenceChange(base, 'Training is slow. ' + base))
            .toEqual({ kind: 'prepend', added: 18 });
    });

    it('头部被删 = 撤销并入上文（必须走到 shrink 分支，不能当成整句替换）', () => {
        expect(classifySentenceChange('Training is slow. ' + base, base))
            .toEqual({ kind: 'shrink-start', removed: 18 });
    });

    it('尾部被删 = 撤销并入下文', () => {
        expect(classifySentenceChange(base + ' It runs fast.', base))
            .toEqual({ kind: 'shrink-end', removed: 14 });
    });

    it('两边都不一样 = 整句替换（重新取词、编辑）', () => {
        expect(classifySentenceChange(base, 'A completely different sentence.'))
            .toEqual({ kind: 'replace' });
    });

    it('空字符串开头（首次上屏）不算并入', () => {
        expect(classifySentenceChange('', base)).toEqual({ kind: 'replace' });
    });
});

describe('remapTokenKeys', () => {
    /** 每次调用返回一个新 key，便于断言哪些是“沿用”的 */
    const counter = () => {
        let next = 100;
        return () => next++;
    };
    const keysOf = (oldTexts: string[], oldKeys: number[], newTexts: string[]) =>
        remapTokenKeys(
            oldTexts,
            oldKeys,
            newTexts,
            classifySentenceChange(oldTexts.join(''), newTexts.join('')),
            counter(),
        );

    it('并入下文时，句末的句号沿用原 key（它只是把后面的空格并进了自己）', () => {
        const oldTexts = ['model', ' ', 'training', '.'];
        const newTexts = ['model', ' ', 'training', '. ', 'Meanwhile', ',', ' ', 'it', ' ', 'runs', '.'];
        const keys = keysOf(oldTexts, [0, 1, 2, 3], newTexts);
        expect(keys.slice(0, 4)).toEqual([0, 1, 2, 3]);
        expect(new Set(keys).size).toBe(keys.length);
    });

    it('撤销并入下文时，变回单独句号的词元也沿用原 key', () => {
        const oldTexts = ['model', ' ', 'training', '. ', 'Meanwhile', '.'];
        const newTexts = ['model', ' ', 'training', '.'];
        expect(keysOf(oldTexts, [0, 1, 2, 3, 4, 5], newTexts)).toEqual([0, 1, 2, 3]);
    });

    it('整句替换不为任何词元沿用旧 key', () => {
        const keys = keysOf(['old', ' ', 'text'], [0, 1, 2], ['new', ' ', 'words']);
        expect(keys.every(key => key >= 100)).toBe(true);
    });
});
