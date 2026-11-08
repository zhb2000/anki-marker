/**
 * 页面操作助手（window.__dev 上的 click / fill / press / text / idle）。
 *
 * 为什么有它：命令行浏览器工具的元素编号每次快照都会变，多条命令之间标签页还可能回到空白页。
 * 用这些助手可以按“看得见的文字”定位元素，把一整段操作写进同一条 eval，
 * 并用 idle() 等到应用真的安静下来，而不是 sleep 猜时间。
 */
import { lastMutationAt, pendingInvokeCount } from './inspect';

const CLICKABLE = 'button, a, [role="button"], [role="radio"], [role="switch"], [role="option"], [role="tab"], [role="menuitem"], label, summary';
const FIELDS = 'input:not([type="checkbox"]):not([type="radio"]), textarea';

function visible(el: Element): boolean {
    return el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';
}

function labelOf(el: Element): string {
    const html = el as HTMLElement;
    return (html.innerText ?? el.textContent ?? '').trim().replace(/\s+/g, ' ');
}

/** 给歧义报错用的描述：名称（含 aria-label / title）+ 所在区块开头的文字，便于区分多个同名按钮 */
function describeHit(el: HTMLElement, attrs: string[]): string {
    const name = [labelOf(el), ...attrs.map(attr => el.getAttribute(attr) ?? '')].find(text => text.length > 0) ?? '(无文字)';
    // 向上找到第一个比自身文字更丰富的祖先，取其开头文字作为“它在哪”的线索
    let context = '';
    for (let node = el.parentElement; node != null && node !== document.body; node = node.parentElement) {
        const text = (node.innerText ?? '').trim().replace(/\s+/g, ' ');
        if (text.length > name.length + 8) {
            context = text.slice(0, 40);
            break;
        }
    }
    return context.length > 0 ? `${name} @ “${context}”` : name;
}

/**
 * 按文字找元素：先精确匹配，再包含匹配；找不到时报错并列出当前可点的文字，AI 能据此自我纠正。
 * 有多个匹配时必须用 nth（从 0 开始，按文档顺序）指定，否则报错并列出各候选的上下文。
 */
function find(target: string, selector: string, attrs: string[], nth?: number): HTMLElement {
    const candidates = [...document.querySelectorAll<HTMLElement>(selector)].filter(visible);
    const names = (el: HTMLElement) => [labelOf(el), ...attrs.map(attr => el.getAttribute(attr) ?? '')].filter(name => name.length > 0);
    const exact = candidates.filter(el => names(el).some(name => name === target));
    const loose = candidates.filter(el => names(el).some(name => name.includes(target)));
    const hits = exact.length > 0 ? exact : loose;
    if (hits.length === 0) {
        try {
            const bySelector = document.querySelector<HTMLElement>(target);
            if (bySelector != null) return bySelector;
        } catch { /* 不是合法选择器 */ }
        const available = [...new Set(candidates.flatMap(names))].slice(0, 40);
        throw new Error(`找不到“${target}”。当前可见的候选：${JSON.stringify(available)}`);
    }
    if (nth !== undefined) {
        if (nth < 0 || nth >= hits.length) {
            throw new Error(`“${target}”只匹配到 ${hits.length} 个元素，nth=${nth} 越界`);
        }
        return hits[nth];
    }
    if (hits.length > 1) {
        const list = hits.map((el, index) => `[${index}] ${describeHit(el, attrs)}`);
        throw new Error(`“${target}”匹配到 ${hits.length} 个元素，请传 nth（如 click('${target}', 0)），或换更具体的文字 / CSS 选择器。候选：${JSON.stringify(list)}`);
    }
    return hits[0];
}

/** 点击：按可见文字、aria-label 或 title 定位（也接受 CSS 选择器），有多个同名元素时用 nth 指定，返回被点元素的描述 */
export function click(target: string, nth?: number): string {
    const el = find(target, CLICKABLE, ['aria-label', 'title'], nth);
    el.click();
    return `clicked <${el.tagName.toLowerCase()}> “${labelOf(el) || el.getAttribute('aria-label') || target}”`;
}

/** 填写输入框：按 placeholder、aria-label 或关联的 label 文字定位（也接受 CSS 选择器），会触发 Vue v-model 需要的 input 事件 */
export function fill(target: string, value: string, nth?: number): string {
    const el = find(target, FIELDS, ['placeholder', 'aria-label', 'name'], nth) as HTMLInputElement | HTMLTextAreaElement;
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return `filled “${el.placeholder || el.getAttribute('aria-label') || target}”`;
}

/** 在目标元素（缺省为当前焦点元素）上按一次键，如 press('Enter')、press('Escape') */
export function press(key: string, target?: string): string {
    const el = target != null ? find(target, FIELDS + ', ' + CLICKABLE, ['placeholder', 'aria-label']) : (document.activeElement as HTMLElement | null) ?? document.body;
    el.focus();
    for (const type of ['keydown', 'keyup']) {
        el.dispatchEvent(new KeyboardEvent(type, { key, code: key, bubbles: true, cancelable: true }));
    }
    return `pressed ${key} on <${el.tagName.toLowerCase()}>`;
}

/** 当前页面可见文字（压缩空白），可限定最大长度 */
export function text(max = 1500): string {
    const content = (document.body.innerText ?? '').replace(/\s+/g, ' ').trim();
    return content.length > max ? `${content.slice(0, max)}…(+${content.length - max})` : content;
}

/**
 * 等应用安静下来：没有进行中的 Rust 命令/HTTP 请求，且页面 DOM 连续 quietMs 毫秒没有变化。
 * 超时不抛错，返回 { settled: false }，由调用方决定是否继续（例如 AI 流式输出本来就很慢）。
 */
export async function idle(options: { quietMs?: number; timeoutMs?: number; } = {}): Promise<{ settled: boolean; waitedMs: number; }> {
    const quietMs = options.quietMs ?? 400;
    const timeoutMs = options.timeoutMs ?? 15000;
    const start = Date.now();
    // 先让出一拍：刚触发的操作需要时间发出第一个 invoke
    await new Promise(resolve => setTimeout(resolve, 60));
    for (;;) {
        const quiet = pendingInvokeCount() === 0 && Date.now() - lastMutationAt() >= quietMs;
        if (quiet) return { settled: true, waitedMs: Date.now() - start };
        if (Date.now() - start >= timeoutMs) return { settled: false, waitedMs: Date.now() - start };
        await new Promise(resolve => setTimeout(resolve, 50));
    }
}

/**
 * 等页面可见文字里出现某段文字（字符串=包含匹配，也可传正则），用于 idle() 看不到的“定时器式等待”，
 * 例如添加笔记时 Anki 连不上的重试与宽限期，最后才弹出错误对话框。超时不抛错，返回 found: false。
 */
export async function waitFor(pattern: string | RegExp, options: { timeoutMs?: number; } = {}): Promise<{ found: boolean; waitedMs: number; }> {
    const timeoutMs = options.timeoutMs ?? 30000;
    const start = Date.now();
    const matches = () => {
        const content = (document.body.innerText ?? '').replace(/\s+/g, ' ');
        return typeof pattern === 'string' ? content.includes(pattern) : pattern.test(content);
    };
    for (;;) {
        if (matches()) return { found: true, waitedMs: Date.now() - start };
        if (Date.now() - start >= timeoutMs) return { found: false, waitedMs: Date.now() - start };
        await new Promise(resolve => setTimeout(resolve, 100));
    }
}
