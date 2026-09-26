#!/usr/bin/env node
/**
 * 由源 SVG 生成托盘图标 PNG。
 *
 * 产物（src-tauri/icons/tray/）：
 *   mono-white-{16,20,24,32}.png  单色白稿（深色任务栏/面板用）
 *   mono-black-{16,20,24,32}.png  单色黑稿（浅色任务栏/面板用）
 *   tray-icon.png                 macOS 菜单栏模板图标（36px = 18pt @2x）
 *
 * 为什么要四档尺寸：Windows 托盘按 DPI 取 SM_CXSMICON（100% = 16px，150% = 20px，
 * 200% = 32px 附近），单一尺寸交由系统缩放会发虚；16px 那档尤其不能靠缩放出图。
 *
 * 源稿（全平台统一）：src-tauri/icons/tray/tray-icon-mono.svg（viewBox 16 单位，
 * 1 单位 = 1px @16px），其中的 currentColor 会被替换为下述目标颜色。
 * macOS 的 36px（18pt @2x）与 mono 16 网格按 2.25 等比渲染在数学上等价，
 * 故不再有独立的 18pt 源稿；若日后 macOS 需要平台专属的视觉微调（如相对占比），
 * 在 MACOS_WIDTH 上做文章即可。
 *
 * 用法：npm run build:tray-icons
 */
import { Resvg } from '@resvg/resvg-js';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const TRAY_DIR = path.join(__dirname, 'src-tauri/icons/tray');
const SOURCE_SVG = path.join(TRAY_DIR, 'tray-icon-mono.svg');

/** 目标颜色：白稿用于深色背景，黑稿用于浅色背景 */
const COLORS = { white: '#ffffff', black: '#000000' };

/** 目标尺寸：覆盖 100% / 150% / 200% DPI 及更高的托盘槽位 */
const SIZES = [16, 20, 24, 32];

const svg = fs.readFileSync(SOURCE_SVG, 'utf8');
if (!svg.includes('currentColor')) {
    throw new Error(`${SOURCE_SVG} 中未找到 currentColor 占位色，无法按颜色导出`);
}

for (const [name, color] of Object.entries(COLORS)) {
    const colored = svg.replaceAll('currentColor', color);
    for (const size of SIZES) {
        const png = new Resvg(colored, {
            fitTo: { mode: 'width', value: size },
            // 托盘图标需要透明背景（由宿主决定底色）
            background: undefined,
        }).render().asPng();
        const outPath = path.join(TRAY_DIR, `mono-${name}-${size}.png`);
        fs.writeFileSync(outPath, png);
        console.log(`wrote ${path.relative(__dirname, outPath)} (${size}x${size})`);
    }
}

// macOS 菜单栏模板图标：18pt @2x = 36px，纯黑+alpha，系统自适配明暗/高亮。
// 与 mono 稿同源（currentColor 换纯黑），直接按 36px 渲染。
const MACOS_WIDTH = 36;
const macPng = new Resvg(svg.replaceAll('currentColor', COLORS.black), {
    fitTo: { mode: 'width', value: MACOS_WIDTH },
    background: undefined,
}).render().asPng();
const macOutPath = path.join(TRAY_DIR, 'tray-icon.png');
fs.writeFileSync(macOutPath, macPng);
console.log(`wrote ${path.relative(__dirname, macOutPath)} (${MACOS_WIDTH}x${MACOS_WIDTH})`);
