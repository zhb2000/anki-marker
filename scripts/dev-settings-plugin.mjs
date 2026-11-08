import fs from 'node:fs';
import path from 'node:path';

/** 开发者设置文件（项目根目录，被 *.local.* 规则 gitignore） */
const DEV_SETTINGS_FILE = 'dev-settings.local.json';
const DEV_SETTINGS_ROUTE = '/__dev/settings';

/**
 * 开发者设置的读写接口，仅 dev server 且总开关打开时挂载（见 docs/dev-features.md）。
 *
 * - GET  /__dev/settings：返回文件内容（不存在则 {}）；
 * - PUT  /__dev/settings：键级合并，值为 null 的键删除；按 src/dev/schema.ts 校验，非法即整体拒绝；
 * - 文件被外部修改时通过 HMR 通道通知页面（`dev-settings-changed`）。
 *
 * 安全：只读写这一个固定文件；Host 必须是本机（防 DNS 重绑定）；PUT 必须带自定义头
 * `X-Dev-Settings`，触发跨域预检，其他网页无法跨站改写。
 */
export function devSettingsPlugin(root) {
    const file = path.resolve(root, DEV_SETTINGS_FILE);
    let writeChain = Promise.resolve();

    const readFile = () => {
        try {
            const data = JSON.parse(fs.readFileSync(file, 'utf8'));
            return typeof data === 'object' && data !== null && !Array.isArray(data)
                ? data
                : {};
        } catch {
            return {};
        }
    };

    const isLocalHost = (host) =>
        host != null && /^(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/.test(host);

    return {
        name: 'anki-marker-dev-settings',
        apply: 'serve',
        configureServer(server) {
            server.watcher.add(file);
            server.watcher.on('change', changed => {
                if (path.resolve(changed) === file) {
                    server.ws.send({ type: 'custom', event: 'dev-settings-changed' });
                }
            });
            server.middlewares.use(DEV_SETTINGS_ROUTE, (req, res) => {
                const reply = (status, body) => {
                    res.statusCode = status;
                    res.setHeader('Content-Type', 'application/json');
                    res.end(JSON.stringify(body));
                };
                if (!isLocalHost(req.headers.host)) {
                    return reply(403, { error: 'forbidden host' });
                }
                if (req.method === 'GET') {
                    return reply(200, readFile());
                }
                if (req.method !== 'PUT') {
                    return reply(405, { error: 'method not allowed' });
                }
                if (req.headers['x-dev-settings'] !== '1') {
                    return reply(403, { error: 'missing X-Dev-Settings header' });
                }
                let raw = '';
                req.on('data', chunk => {
                    raw += String(chunk);
                    if (raw.length > 64 * 1024) {
                        req.destroy();
                    }
                });
                req.on('end', () => {
                    void (async () => {
                        let patch;
                        try {
                            const parsed = JSON.parse(raw);
                            if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
                                throw new Error('body must be an object');
                            }
                            patch = parsed;
                        } catch (error) {
                            return reply(400, { error: String(error) });
                        }
                        const schema = await server.ssrLoadModule('/src/dev/schema.ts');
                        for (const [key, value] of Object.entries(patch)) {
                            if (!schema.isDevSettingKey(key)) {
                                return reply(400, { error: `unknown key: ${key}` });
                            }
                            if (value !== null && schema.coerceDevSetting(key, value) === undefined) {
                                return reply(400, { error: `invalid value for ${key}` });
                            }
                        }
                        // 串行化：并发的读-改-写不能交错；先写临时文件再改名，避免写一半被读到
                        writeChain = writeChain.then(() => {
                            const merged = { ...readFile() };
                            for (const [key, value] of Object.entries(patch)) {
                                if (value === null) {
                                    delete merged[key];
                                } else {
                                    merged[key] = value;
                                }
                            }
                            const tmp = `${file}.tmp`;
                            fs.writeFileSync(tmp, JSON.stringify(merged, null, 2) + '\n');
                            fs.renameSync(tmp, file);
                            reply(200, merged);
                        }).catch((error) => reply(500, { error: String(error) }));
                    })();
                });
            });
        },
    };
}
