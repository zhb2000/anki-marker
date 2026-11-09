/**
 * 卡片模板的构建与 Vite 插件。
 *
 * 产物不是本应用的一部分：它是一段要经 AnkiConnect 发给 Anki 的自包含字符串。
 * 因此这里把三类源码编译成一份：
 *   - script.ts   → 单个 IIFE（共享模块 src/shared 一并打包，保证换卡时重复执行不出错）
 *   - style.css   → 把 url(./icons/x.png) 换成 data URL，图标以普通文件维护
 *   - front.html  → 把 <!-- card-script --> 换成内联脚本
 *
 * 开发期由 Vite 的 load 钩子按需构建并缓存；改动下面的源码会失效缓存并整页重载。
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { rolldown } from 'rolldown';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SOURCE_DIR = path.join(ROOT, 'src', 'card-template');

export const VIRTUAL_ID = 'virtual:card-template';
const RESOLVED_ID = '\0' + VIRTUAL_ID;

const MIME_BY_EXT = { '.png': 'image/png', '.svg': 'image/svg+xml' };

/**
 * 构建期的依赖白名单：卡片脚本只允许引用 src/shared 与 src/card-template 下的模块。
 * 这条约束不能只写在注释里——一旦有人 import 了 logics/youdao.ts，
 * Tauri 的 net 层就会被一起打进产物，开发时看不出来，只有 Anki 里整段脚本崩掉。
 */
function purityGuard() {
    return {
        name: 'card-template-purity',
        resolveId(source, importer) {
            if (importer == null) {
                return null; // 入口文件自己
            }
            if (!source.startsWith('.')) {
                throw new Error('卡片脚本不能引入外部模块：' + source + '（' + importer + '）');
            }
            const rel = path.relative(ROOT, path.resolve(path.dirname(importer), source))
                .split(path.sep).join('/');
            if (!rel.startsWith('src/shared/') && !rel.startsWith('src/card-template/')) {
                throw new Error('卡片脚本不能依赖 ' + rel + '：它会把应用运行时（Tauri/dev）带进 Anki');
            }
            return null;
        },
    };
}

/** 打包卡片脚本；format 为 iife，因此产物整体在一个作用域里，重复执行安全 */
async function buildScript() {
    const bundle = await rolldown({
        input: path.join(SOURCE_DIR, 'script.ts'),
        platform: 'browser',
        transform: {
            // 卡片运行在 Anki 的 WebView 里：桌面为 Chromium、移动端为 WKWebView/Android WebView
            target: 'es2020',
            // 纯模块里若误用了编译期常量，这里给一个兜底值；正常情况不应该出现
            define: { __DEV_FEATURES__: 'false' },
        },
        plugins: [purityGuard()],
        onwarn() { /* 构建告警不影响产物，保持安静 */ },
    });
    try {
        const { output } = await bundle.generate({ format: 'iife' });
        const chunk = output.find(item => item.type === 'chunk');
        if (chunk == null) {
            throw new Error('卡片脚本没有产出 chunk');
        }
        return chunk.code;
    } finally {
        await bundle.close();
    }
}

/** 把 CSS 里的相对资源引用换成 data URL：产物必须自包含，Anki 里没有相对路径可解析 */
function inlineAssets(css) {
    return css.replace(/url\(\s*['"]?([^'")]+)['"]?\s*\)/g, (whole, ref) => {
        if (ref.startsWith('data:') || ref.startsWith('http')) {
            return whole;
        }
        const file = path.resolve(SOURCE_DIR, ref);
        const mime = MIME_BY_EXT[path.extname(file).toLowerCase()];
        if (mime == null) {
            throw new Error('无法内联的静态资源：' + ref);
        }
        if (!fs.existsSync(file)) {
            throw new Error('找不到静态资源：' + ref);
        }
        return 'url("data:' + mime + ';base64,' + fs.readFileSync(file).toString('base64') + '")';
    });
}

/** 产物里不能出现这些：要么会被 Anki 的模板引擎吃掉，要么说明依赖没有拦住 */
function assertArtifact(script, css) {
    for (const [label, text] of [['脚本', script], ['样式', css]]) {
        if (text.includes('{{')) {
            throw new Error(label + '产物里出现了两个左花括号：Anki 会当成字段引用，必须先转义');
        }
    }
    for (const forbidden of ['@tauri-apps', '__TAURI_INTERNALS__', '__DEV_FEATURES__']) {
        if (script.includes(forbidden)) {
            throw new Error('卡片脚本里出现了 ' + forbidden + '：它会把应用运行时带进 Anki');
        }
    }
    if (/url\(\s*['"]?\.\//.test(css)) {
        throw new Error('样式里还有相对路径资源未被内联');
    }
}

/** 版本号写在 front.html 的 JSON 块里，外部工具也靠它识别模板 */
function readVersion(front) {
    const match = /<script[^>]*id="com\.zhb2000\.anki-marker_card-template-info"[^>]*>([\s\S]*?)<\/script>/.exec(front);
    if (match == null) {
        throw new Error('front.html 缺少模板信息块');
    }
    const info = JSON.parse(match[1]);
    if (typeof info.version !== 'string') {
        throw new Error('模板信息块里没有 version');
    }
    return info.version;
}

export async function buildCardTemplate() {
    const script = await buildScript();
    const css = inlineAssets(fs.readFileSync(path.join(SOURCE_DIR, 'style.css'), 'utf8'));
    const frontSource = fs.readFileSync(path.join(SOURCE_DIR, 'front.html'), 'utf8');
    const back = fs.readFileSync(path.join(SOURCE_DIR, 'back.html'), 'utf8');

    if (!frontSource.includes('<!-- card-script -->')) {
        throw new Error('front.html 缺少 <!-- card-script --> 占位');
    }
    const front = frontSource.replace('<!-- card-script -->', () => '<script>\n' + script + '</script>');

    assertArtifact(script, css);
    return { front, back, css, version: readVersion(front) };
}

export function cardTemplatePlugin() {
    let cache = null;
    const build = async () => (cache ??= await buildCardTemplate());

    return {
        name: 'card-template',
        resolveId(id) {
            return id === VIRTUAL_ID ? RESOLVED_ID : null;
        },
        async load(id) {
            if (id !== RESOLVED_ID) {
                return null;
            }
            const { front, back, css, version } = await build();
            return [
                'export const front = ' + JSON.stringify(front) + ';',
                'export const back = ' + JSON.stringify(back) + ';',
                'export const css = ' + JSON.stringify(css) + ';',
                'export const version = ' + JSON.stringify(version) + ';',
            ].join('\n');
        },
        configureServer(server) {
            // 模板源码或共享模块变了：丢掉缓存、让虚拟模块失效，再整页重载。
            // 预览页因此仍然保持“保存即刷新”，只是刷新粒度是整页而不是热替换。
            const onChange = (file) => {
                const rel = path.relative(ROOT, file).split(path.sep).join('/');
                if (!rel.startsWith('src/card-template/') && !rel.startsWith('src/shared/')) {
                    return;
                }
                cache = null;
                const mod = server.moduleGraph.getModuleById(RESOLVED_ID);
                if (mod != null) {
                    server.moduleGraph.invalidateModule(mod);
                }
                server.ws.send({ type: 'full-reload' });
            };
            server.watcher.on('change', onChange);
            server.watcher.on('add', onChange);
            server.watcher.on('unlink', onChange);
        },
    };
}
