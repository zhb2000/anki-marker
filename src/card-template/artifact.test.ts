import { beforeAll, describe, expect, it, vi } from 'vitest';

import { front } from 'virtual:card-template';

/** 取出正面模板末尾内联的卡片脚本（前面的信息块不参与执行） */
function extractScript(html: string): { markup: string; script: string; } {
    const open = html.lastIndexOf('<script>');
    const close = html.lastIndexOf('</script>');
    return {
        markup: html.slice(0, open),
        script: html.slice(open + '<script>'.length, close),
    };
}

interface CardState { generation: number; }

describe('卡片模板产物', () => {
    // jsdom 没有实现媒体播放，换卡时会调用 pause()，这里替掉以免刷屏
    beforeAll(() => {
        vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => { /* noop */ });
    });

    /**
     * 真机上换卡与翻面都会重新执行整段脚本，因此产物必须整体处在一个作用域里。
     * 这条同时兜住两件事：打包格式被改成非 IIFE，以及依赖里混进了应用运行时。
     */
    it('在只有 WebView 的环境里可以连续执行两次', () => {
        const { markup, script } = extractScript(front);
        document.body.innerHTML = markup;

        expect(() => {
            window.eval(script);
            window.eval(script);
        }).not.toThrow();

        const state = (window as unknown as { __ankiMarkerCardState?: CardState }).__ankiMarkerCardState;
        expect(state?.generation).toBe(2);
    });

    /**
     * 模板里的展开/收起是内联 onclick 调用的，函数必须挂在 window 上；
     * 打包成 IIFE 后很容易把这两个赋值也一起“作用域化”，届时卡片一点反应都没有。
     */
    it('内联 onclick 用到的函数挂在 window 上', () => {
        const { markup, script } = extractScript(front);
        document.body.innerHTML = markup;
        window.eval(script);

        const api = window as unknown as { toggle: (id: string) => void; clickSentenceButton: () => Promise<void>; };
        expect(typeof api.toggle).toBe('function');
        expect(typeof api.clickSentenceButton).toBe('function');

        api.toggle('sentence-grid');
        expect(document.getElementById('sentence-grid')?.classList.contains('fold')).toBe(true);
    });
});
