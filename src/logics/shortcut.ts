/** 将快捷键字符串（如 "Cmd+Shift+KeyS"）格式化为 macOS 显示形式（如 "⌘ ⇧ S"） */
export function formatShortcut(shortcut: string): string {
    return shortcut
        .split('+')
        .map(token => {
            switch (token) {
                case 'Cmd': return '⌘';
                case 'Ctrl': return '⌃';
                case 'Alt': return '⌥';
                case 'Shift': return '⇧';
                default: return codeToDisplay(token);
            }
        })
        .join(' ');
}

/** 将键名（如 "KeyS"、"Digit1"）转换为显示形式（如 "S"、"1"） */
function codeToDisplay(code: string): string {
    if (/^Key[A-Z]$/.test(code)) {
        return code.slice(3);
    }
    if (/^Digit\d$/.test(code)) {
        return code.slice(5);
    }
    switch (code) {
        case 'Space': return '空格';
        case 'Comma': return ',';
        case 'Period': return '.';
        case 'Slash': return '/';
        case 'Backslash': return '\\';
        case 'Minus': return '-';
        case 'Equal': return '=';
        case 'Semicolon': return ';';
        case 'Quote': return '\'';
        case 'BracketLeft': return '[';
        case 'BracketRight': return ']';
        case 'Backquote': return '`';
        default: return code;
    }
}
