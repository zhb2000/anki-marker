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
            tokens.push(text.substring(lastEnd, start)); // push previous non-word token
        }
        tokens.push(token); // push current word token
        lastEnd = start + token.length;
    }
    if (lastEnd < text.length) {
        tokens.push(text.substring(lastEnd)); // push the last non-word token if exists
    }
    return tokens;
}

const __escapeDiv = document.createElement('div');

export function escapeHTML(html: string): string {
    __escapeDiv.textContent = html; // 使用 textContent 而不是 innerHTML 来避免 HTML 解析
    return __escapeDiv.innerHTML; // 获取转义后的 HTML 字符串
}
