import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

/** Rust 侧 dev-bridge 启动后写入的连接信息（项目根目录，gitignore） */
export const BRIDGE_INFO_FILE = 'dev-bridge.local.json';
const ROUTE = '/__dev/bridge';

/**
 * 把浏览器的 /__dev/bridge/* 代理到 Rust 的 dev-bridge（src-tauri/src/application/dev_bridge.rs）。
 *
 * - 令牌只在这里读取并注入，浏览器端拿不到；
 * - GET /__dev/bridge/status：桥接是否在运行（读信息文件并确认进程存活）；
 * - 其余请求原样转发（含 SSE 流式响应）；
 * - 安全：Host 必须是本机；所有请求必须带自定义头 X-Dev-Bridge，跨站网页发不出带该头的
 *   请求（预检会失败），所以其他网页无法借浏览器操作本机的 Rust 命令。
 */
export function devBridgeProxyPlugin(root) {
    const infoFile = path.resolve(root, BRIDGE_INFO_FILE);

    const readInfo = () => {
        try {
            const info = JSON.parse(fs.readFileSync(infoFile, 'utf8'));
            process.kill(info.pid, 0); // 进程不存在会抛错
            return info;
        } catch {
            return null;
        }
    };
    const isLocalHost = (host) => host != null && /^(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/.test(host);

    return {
        name: 'anki-marker-dev-bridge-proxy',
        apply: 'serve',
        configureServer(server) {
            server.middlewares.use(ROUTE, (req, res) => {
                const reply = (status, body) => {
                    res.statusCode = status;
                    res.setHeader('Content-Type', 'application/json');
                    res.end(JSON.stringify(body));
                };
                if (!isLocalHost(req.headers.host)) {
                    return reply(403, { error: 'forbidden host' });
                }
                if (req.headers['x-dev-bridge'] !== '1') {
                    return reply(403, { error: 'missing X-Dev-Bridge header' });
                }
                const info = readInfo();
                if (req.url === '/status') {
                    // root：这个 Vite 属于哪个 worktree，供 dev-ai 判断“占用端口的是不是自己”
                    return reply(200, { available: info != null, root: path.resolve(root) });
                }
                if (info == null) {
                    return reply(503, { error: 'dev-bridge is not running (start the app with: npm run dev:ai)' });
                }
                const headers = { ...req.headers, 'x-dev-bridge-token': info.token, host: `127.0.0.1:${info.port}` };
                delete headers.origin;
                delete headers.referer;
                const upstream = http.request(
                    { host: '127.0.0.1', port: info.port, method: req.method, path: req.url, headers },
                    (upstreamRes) => {
                        res.writeHead(upstreamRes.statusCode ?? 502, upstreamRes.headers);
                        upstreamRes.pipe(res);
                    },
                );
                upstream.on('error', (error) => {
                    if (!res.headersSent) {
                        reply(502, { error: String(error) });
                    } else {
                        res.end();
                    }
                });
                res.on('close', () => upstream.destroy());
                req.pipe(upstream);
            });
        },
    };
}
