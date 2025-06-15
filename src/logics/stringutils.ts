export function isWord(token: string): boolean {
    return /^\w+$/.test(token);
}

export function tokenize(text: string): string[] {
    const tokens = [];
    const regex = /\w+/g;
    let lastEnd = 0;
    for (let match = regex.exec(text); match != null; match = regex.exec(text)) {
        const token = match[0];
        const start = match.index;
        if (lastEnd < start) {
            tokens.push(text.slice(lastEnd, start)); // push previous non-word token
        }
        tokens.push(token); // push current word token
        lastEnd = start + token.length;
    }
    if (lastEnd < text.length) {
        tokens.push(text.slice(lastEnd)); // push the last non-word token if exists
    }
    return tokens;
}

const escapeDiv = document.createElement('div');

/**
 * 转义 HTML 字符串中的特殊字符。
 * 
 * 示例：`escapeHTML('<script>alert("XSS")</script>')` 返回 `&lt;script&gt;alert(&quot;XSS&quot;)&lt;/script&gt;`
 */
export function escapeHTML(html: string): string {
    escapeDiv.textContent = html; // 使用 textContent 而不是 innerHTML 来避免 HTML 解析
    return escapeDiv.innerHTML; // 获取转义后的 HTML 字符串
}

const decodeTextarea = document.createElement('textarea');

/**
 * 解码 HTML 字符串中的实体字符。
 * 
 * 示例：`decodeHtmlEntities('&lt;script&gt;alert(&quot;XSS&quot;)&lt;/script&gt;')` 返回 `<script>alert("XSS")</script>`
 */
export function decodeHtmlEntities(content: string): string {
    decodeTextarea.innerHTML = content;
    return decodeTextarea.value;
}
