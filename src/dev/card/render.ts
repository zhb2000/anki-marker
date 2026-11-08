/**
 * Anki 卡片模板的极简求值器，只覆盖本项目模板用到的语法：
 * {{字段}}、{{#字段}}…{{/字段}}、{{^字段}}…{{/字段}}、{{text:字段}}、{{FrontSide}}。
 *
 * 它只为开发期看效果服务，不是 Anki 模板引擎的复刻。真机仍有差异：
 * Anki 注入的基础样式、音频自动播放、{{hint:}}/{{type:}} 等语法。
 * 发布前仍需要在 Anki 里过一眼。
 */

/** 待展开的 {{FrontSide}} 先换成占位符，避免正面内容被再求值一次 */
const FRONT_SIDE_TOKEN = '\u0000front-side\u0000';

/** 形如 {{#例句}}…{{/例句}} 的区块；嵌套同名区块不支持，本项目模板没有用到 */
const SECTION_RE = /\{\{([#^])([^{}]+?)\}\}([\s\S]*?)\{\{\/([^{}]+?)\}\}/g;

/** 去掉标签后的纯文本，用于判断字段是否为空、以及 {{text:字段}} */
export function stripHtml(html: string): string {
    return html.replace(/<[^>]*>/g, '').replace(/&nbsp;/g, ' ');
}

/**
 * Anki 对“空字段”的判定：去掉标签与空白后为空；只有图片的字段算非空
 * （否则 {{#字段}} 会把一张只有配图的条目吞掉）
 */
function isFieldEmpty(value: string): boolean {
    if (/<img\b/i.test(value)) {
        return false;
    }
    return stripHtml(value).trim() === '';
}

function expandSections(template: string, fields: Record<string, string>): string {
    let out = template;
    for (;;) {
        let changed = false;
        SECTION_RE.lastIndex = 0;
        out = out.replace(SECTION_RE, (whole, sigil: string, name: string, inner: string, closeName: string) => {
            if (name.trim() !== closeName.trim()) {
                return whole;
            }
            changed = true;
            const empty = isFieldEmpty(fields[name.trim()] ?? '');
            return (sigil === '#' ? !empty : empty) ? inner : '';
        });
        if (!changed) {
            return out;
        }
    }
}

/**
 * 渲染一个模板。
 *
 * @param template 正面或背面模板
 * @param fields 字段名到 HTML 的映射（字段内容按 Anki 的习惯当 HTML 插入）
 * @param frontSide 背面模板用：已渲染好的正面 HTML，供 {{FrontSide}} 展开
 */
export function renderTemplate(
    template: string,
    fields: Record<string, string>,
    frontSide?: string
): string {
    let out = template.includes('{{FrontSide}}')
        ? template.replace(/\{\{FrontSide\}\}/g, FRONT_SIDE_TOKEN)
        : template;
    out = expandSections(out, fields);
    out = out.replace(/\{\{text:([^{}]+?)\}\}/g, (_, name: string) => stripHtml(fields[name.trim()] ?? ''));
    out = out.replace(/\{\{([^{}]+?)\}\}/g, (_, name: string) => fields[name.trim()] ?? '');
    // Anki 会把 [sound:foo.mp3] 换成自带的播放按钮；这里给一个形状相近的替代，
    // 好让 {{发音}} 在版面里占据与真机接近的宽度。不带 href：预览不放音，
    // 也不希望点一下就把 iframe 导航走
    out = out.replace(/\[sound:([^\]]+)\]/g, (_, name: string) =>
        `<a class="replay-button soundLink" title="预览不播放：${name.replace(/"/g, '&quot;')}">▶</a>`);
    return frontSide == null ? out : out.split(FRONT_SIDE_TOKEN).join(frontSide);
}
