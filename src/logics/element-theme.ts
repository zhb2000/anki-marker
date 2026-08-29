/**
 * element-plus 主题色生成。
 *
 * - 修改主题色：https://github.com/element-plus/element-plus/discussions/14659
 * - 主题：https://element-plus.org/zh-CN/guide/theming.html
 * - 暗色色阶公式：node_modules/element-plus/theme-chalk/dark/css-vars.css
 *
 * 独立成模块以打破 globals ↔ theme 的循环依赖（globals 与 theme 都需要 setElementTheme），
 * 使依赖保持单向：globals → theme → element-theme。
 */

import 'element-plus/theme-chalk/dark/css-vars.css';

import * as utils from './utils';

/**
 * 设置 element-plus 主题色
 *
 * 每次主题切换后都需要调用（`--accent` 的值可能随暗色模式改变，必须重新读取）。
 */
export function setElementTheme() {
    const root: HTMLElement = document.documentElement;
    const isDark = root.classList.contains('dark');
    const styles = getComputedStyle(root);
    const accentColor = styles.getPropertyValue('--accent').trim();
    setThemeColor(accentColor, isDark);
}

/** 调整亮度生成 element-plus 主题色的 light 颜色（浅色模式） */
function adjustLightness(h: number, s: number, l: number, adjustment: number): string {
    return `hsl(${h}, ${s}%, ${Math.min(l + adjustment, 100)}%)`;
}

interface RgbColor {
    r: number;
    g: number;
    b: number;
}

/**
 * 按 element-plus 官方公式混色：mix(mixColor, primary, level)
 * 即 level% 的 mixColor 与 (100 - level)% 的 primary 线性混合
 */
function mixWithPrimary(primary: RgbColor, mixColor: RgbColor, level: number): string {
    const weight = level / 100;
    const r = Math.round((mixColor.r * weight + primary.r * (1 - weight)) * 10) / 10;
    const g = Math.round((mixColor.g * weight + primary.g * (1 - weight)) * 10) / 10;
    const b = Math.round((mixColor.b * weight + primary.b * (1 - weight)) * 10) / 10;
    return `rgb(${r}, ${g}, ${b})`;
}

function setThemeColor(primaryColor: string, isDark: boolean): void {
    const root: HTMLElement = document.documentElement;
    // 获取 HSL 值
    const { r, g, b } = utils.hexToRgb(primaryColor);
    const { h, s, l } = utils.rgbToHsl(r, g, b);
    const primary: RgbColor = { r, g, b };
    // 设置主色
    root.style.setProperty('--el-color-primary', `hsl(${h}, ${s}%, ${l}%)`);
    if (isDark) {
        // 暗色模式：light-N 为 primary 与暗色背景 #141414 按 N*10% 的混色，
        // dark-2 为 primary 与白色按 20% 的混色（官方暗色公式）
        const darkBackground: RgbColor = { r: 0x14, g: 0x14, b: 0x14 };
        const white: RgbColor = { r: 0xff, g: 0xff, b: 0xff };
        root.style.setProperty('--el-color-primary-light-3', mixWithPrimary(primary, darkBackground, 30));
        root.style.setProperty('--el-color-primary-light-5', mixWithPrimary(primary, darkBackground, 50));
        root.style.setProperty('--el-color-primary-light-7', mixWithPrimary(primary, darkBackground, 70));
        root.style.setProperty('--el-color-primary-light-8', mixWithPrimary(primary, darkBackground, 80));
        root.style.setProperty('--el-color-primary-light-9', mixWithPrimary(primary, darkBackground, 90));
        root.style.setProperty('--el-color-primary-dark-2', mixWithPrimary(primary, white, 20));
    } else {
        // 浅色模式：保持原有 HSL 加亮算法，
        // dark-2 为 primary 与黑色按 20% 的混色（官方浅色公式）
        const black: RgbColor = { r: 0, g: 0, b: 0 };
        root.style.setProperty('--el-color-primary-light-3', adjustLightness(h, s, l, 10));
        root.style.setProperty('--el-color-primary-light-5', adjustLightness(h, s, l, 20));
        root.style.setProperty('--el-color-primary-light-7', adjustLightness(h, s, l, 30));
        root.style.setProperty('--el-color-primary-light-8', adjustLightness(h, s, l, 35));
        root.style.setProperty('--el-color-primary-light-9', adjustLightness(h, s, l, 40));
        root.style.setProperty('--el-color-primary-dark-2', mixWithPrimary(primary, black, 20));
    }
    root.style.setProperty('--el-color-white', '#ffffff');
    root.style.setProperty('--el-color-black', '#000000');
}
