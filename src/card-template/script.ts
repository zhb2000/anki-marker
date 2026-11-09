/**
 * 卡片脚本。构建时用 rolldown 打包成单文件 IIFE，内联进正面模板（见 scripts/card-template.mjs）。
 *
 * 这段代码运行在 Anki 的 WebView 里，不属于应用：只能用标准 Web API，
 * 不能引用 Tauri 或 src/dev（构建期有白名单校验，违规直接构建失败）。
 *
 * 关于重复执行：Anki 换卡与翻面都会重新执行整段脚本（背面的 FrontSide 展开会把正面
 * 脚本再引入一次），所以整个产物必须处在一个作用域里。这件事现在由打包器的 IIFE 保证，
 * 不要在模块顶层放“执行第二次就出错”的东西（例如顶层 const/class 声明）。
 */
import { makeIcibaTtsUrl, makeYoudaoPronounceUrl } from '../shared/tts';

/** 跨次执行共享的状态；挂在 window 上，每次执行取回同一份 */
interface CardState {
    /** 例句朗读用的唯一 Audio，换卡后仍能找到上一张卡正在播放的那个并暂停 */
    audio: HTMLAudioElement;
    /** 唯一的可见性监听，每次执行先断开再重新挂到当前卡片上，避免逐卡累积 */
    observer: MutationObserver | null;
    /** 执行序号，异步查找可用音频期间换了卡则放弃播放 */
    generation: number;
}

const win = window as Window & { __ankiMarkerCardState?: CardState };

const shared = win.__ankiMarkerCardState ?? (win.__ankiMarkerCardState = {
    audio: new Audio(),
    observer: null,
    generation: 0,
});
shared.generation += 1;
// 脚本被重新执行意味着换卡或翻面：上一次的例句朗读不应继续
// （桌面版换卡时虽会把 #qa 的 opacity 置 0，但随即执行的新脚本会断开旧监听，丢掉这条通知）
shared.audio.pause();

/** 展开或收起一个区块 */
function toggle(id: string): void {
    const box = document.getElementById(id);
    if (box != null) {
        box.classList.toggle('fold');
    }
}

const sentenceAudio = shared.audio;

/** 例句区块不可见时暂停朗读 */
function setupSentenceButtonVisibilityObserver(): void {
    // 幂等：先断开上一张卡片挂的监听（它观察的祖先节点在换卡后仍然存在，不断开会逐卡累积）
    if (shared.observer != null) {
        shared.observer.disconnect();
        shared.observer = null;
    }
    const sentenceSection = document.getElementById('sentenceSection');
    if (sentenceSection == null) {
        return; // 本卡没有例句；下一张卡执行脚本时会重新挂接
    }
    const observer = new MutationObserver((mutations) => {
        mutations.forEach(mutation => {
            if (mutation.type !== 'attributes'
                || (mutation.attributeName !== 'style' && mutation.attributeName !== 'class')) {
                return;
            }
            const target = mutation.target;
            if (!(target instanceof Element)) {
                return;
            }
            const computedStyle = window.getComputedStyle(target);
            if (computedStyle.display === 'none'
                || computedStyle.visibility === 'hidden'
                || computedStyle.opacity === '0') {
                sentenceAudio.pause();
            }
        });
    });
    shared.observer = observer;
    observer.observe(sentenceSection, { attributes: true, attributeFilter: ['style', 'class'] });
    // 祖先节点的可见性变化同样会遮住这个区块
    for (let parent = sentenceSection.parentElement; parent != null; parent = parent.parentElement) {
        observer.observe(parent, { attributes: true, attributeFilter: ['style', 'class'] });
    }
}

/** 点击例句喇叭：播放/暂停例句朗读 */
async function clickSentenceButton(): Promise<void> {
    const generation = shared.generation;
    const isPlaying = !sentenceAudio.paused;
    sentenceAudio.pause();
    if (isPlaying) {
        return;
    }
    const sentenceElem = document.getElementById('sentence-content');
    if (sentenceElem == null) {
        return;
    }
    const sentence = sentenceElem.innerText;
    // urls 中的顺序表示优先级（与应用侧 makePronunciationURL 保持一致）
    const urls = [
        makeYoudaoPronounceUrl(sentence),
        makeIcibaTtsUrl(sentence),
        // 这里的 dictvoice 只带 audio 参数、且用 encodeURIComponent 编码，
        // 与 shared/tts 里带 type 参数、用 URLSearchParams 的版本不同，故不复用
        'https://dict.youdao.com/dictvoice?audio=' + encodeURIComponent(sentence),
    ];
    const timeoutMs = 5000;
    const validIndex = await findFirstValidAudio(urls, timeoutMs);
    if (validIndex == null || generation !== shared.generation) {
        return; // 没有可用音频，或查找期间已换卡/翻面
    }
    sentenceAudio.src = urls[validIndex];
    void sentenceAudio.play();
}

/** 探测一个地址能否播放（有 canplay 即认为可用） */
function checkUrlPlayability(url: string, timeoutMs: number): Promise<boolean> {
    return new Promise((resolve) => {
        const tempAudio = document.createElement('audio');
        tempAudio.preload = 'metadata';
        let timeoutId = 0;
        const cleanupAndResolve = (result: boolean) => {
            window.clearTimeout(timeoutId);
            tempAudio.removeEventListener('canplay', canPlayListener);
            tempAudio.removeEventListener('error', errorListener);
            tempAudio.removeEventListener('abort', errorListener);
            tempAudio.src = '';
            resolve(result);
        };
        const canPlayListener = () => cleanupAndResolve(true);
        const errorListener = () => cleanupAndResolve(false);
        tempAudio.addEventListener('canplay', canPlayListener, { once: true });
        tempAudio.addEventListener('error', errorListener, { once: true });
        tempAudio.addEventListener('abort', errorListener, { once: true });
        tempAudio.src = url;
        tempAudio.load();
        timeoutId = window.setTimeout(() => {
            cleanupAndResolve(false);
        }, timeoutMs);
    });
}

/** 并行探测候选地址，按 urls 的顺序取第一个可用的；都不可用返回 null */
function findFirstValidAudio(urls: string[], timeoutMs: number): Promise<number | null> {
    if (urls.length === 0) {
        return Promise.resolve(null);
    }
    return new Promise((resolveOuter) => {
        let isResolved = false;
        const numUrls = urls.length;
        const results = new Array<'valid' | 'invalid' | 'incomplete'>(numUrls).fill('incomplete');
        let completedCount = 0;
        const attemptResolve = () => {
            if (isResolved) {
                return;
            }
            for (let i = 0; i < numUrls; i++) {
                if (results[i] === 'valid') {
                    isResolved = true;
                    resolveOuter(i);
                    return;
                } else if (results[i] === 'incomplete') {
                    return;
                }
            }
            if (completedCount === numUrls) {
                isResolved = true;
                resolveOuter(null);
            }
        };
        void Promise.all(urls.map(async (url, index) => {
            let isPlayable: boolean;
            try {
                isPlayable = await checkUrlPlayability(url, timeoutMs);
            } catch (error) {
                isPlayable = false;
                console.error("检查音频 URL '" + url + "' 时发生意外错误:", error);
            }
            results[index] = isPlayable ? 'valid' : 'invalid';
            completedCount++;
            attemptResolve();
        }));
    });
}

// 供模板中的内联 onclick 调用（正面与背面模板都用到 toggle）
Object.assign(window, { toggle, clickSentenceButton });

// Initialize observer when DOM is ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', setupSentenceButtonVisibilityObserver);
} else {
    setupSentenceButtonVisibilityObserver();
}
