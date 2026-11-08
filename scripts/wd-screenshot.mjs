/**
 * 真机（tauri-plugin-webdriver）截图，带窗口可见性保护。
 *
 * 用法：node scripts/wd-screenshot.mjs [输出路径，默认 /tmp/shot.png]
 *   环境变量 WD_PORT（默认 4445）、WD_SESSION（已有会话 id；缺省则自建会话并在结束时删除）
 *
 * 为什么需要：macOS 的 WKWebView 在窗口被完全遮挡时会把页面当作不可见，暂停重绘，
 * 此时截图拿到的可能是旧画面，AI 据此下结论会出错。截图前先读 document.visibilityState，
 * 不是 'visible' 就拒绝截图（退出码 2），让调用方知道“窗口被挡住了”，而不是悄悄返回旧图。
 *
 * 退出码：0 成功；2 窗口不可见；1 其他错误。
 */
import fs from 'node:fs';

const port = process.env.WD_PORT ?? '4445';
const base = `http://127.0.0.1:${port}`;
const out = process.argv[2] ?? '/tmp/shot.png';

async function call(method, path, body) {
    const response = await fetch(base + path, {
        method,
        headers: { 'Content-Type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
    });
    const data = await response.json();
    if (!response.ok || data.value?.error) {
        throw new Error(`${method} ${path} -> ${response.status} ${JSON.stringify(data.value)}`);
    }
    return data.value;
}

let sid = process.env.WD_SESSION;
const ownSession = sid === undefined;
try {
    if (ownSession) {
        sid = (await call('POST', '/session', { capabilities: { alwaysMatch: {} } })).sessionId;
    }
    const state = await call('POST', `/session/${sid}/execute/sync`, {
        script: 'return { visibility: document.visibilityState, hasFocus: document.hasFocus() }',
        args: [],
    });
    if (state.visibility !== 'visible') {
        console.error(
            `窗口当前不可见（visibilityState=${state.visibility}），多半被其他窗口完全遮挡或已最小化。` +
            '此时截图可能是旧画面，已拒绝。请让用户露出窗口后重试。');
        process.exitCode = 2;
    } else {
        const png = await call('GET', `/session/${sid}/screenshot`);
        fs.writeFileSync(out, Buffer.from(png, 'base64'));
        console.log(`已保存 ${out}（visibilityState=visible, hasFocus=${state.hasFocus}）`);
    }
} catch (error) {
    console.error(String(error));
    process.exitCode = 1;
} finally {
    if (ownSession && sid !== undefined) {
        await call('DELETE', `/session/${sid}`).catch(() => { });
    }
}
