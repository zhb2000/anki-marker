import { os } from '../tauri-api';

/** 是否为 macOS。用函数而非模块级常量：模块求值时 Tauri 运行时可能尚未就绪（浏览器模式的 IPC 打桩在应用模块之前才装好） */
const isMacOS = () => os.type() === 'macos';

/**
 * 将快捷键字符串（如 "Cmd+Shift+KeyS"）格式化为显示形式。
 * macOS 用修饰键字形（如 "⌘ ⇧ S"）；Windows/Linux 用文本式（如 "Ctrl + Shift + S"）。
 * 注：快捷键字符串中的 "Cmd" 在非 macOS 平台即 Win/Super 键
 *（global-hotkey 把 Cmd/Meta/Super 视为同一修饰键，此处只做显示映射）。
 */
export function formatShortcut(shortcut: string): string {
    if (isMacOS()) {
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
    return shortcut
        .split('+')
        .map(token => {
            switch (token) {
                case 'Cmd': return os.type() === 'windows' ? 'Win' : 'Super';
                case 'Ctrl': return 'Ctrl';
                case 'Alt': return 'Alt';
                case 'Shift': return 'Shift';
                default: return codeToDisplay(token);
            }
        })
        .join(' + ');
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
