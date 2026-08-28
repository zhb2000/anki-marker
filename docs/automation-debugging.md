# 应用自动化调试指南（macOS，面向 AI Agent）

本文档说明如何在 macOS 上以编程方式操作本应用（真实运行的 Tauri 应用，非浏览器 mock），
包括截图查看 UI、点击、滚动、执行 JavaScript 等。目标读者是 AI coding agent，
所有操作均可通过 `curl` + `python3` 完成，无需安装 Selenium / WebdriverIO。

## 原理

项目以 Cargo feature 方式集成了 [tauri-plugin-webdriver](https://github.com/Choochmeque/tauri-plugin-webdriver)
（`src-tauri/Cargo.toml` 中的 `webdriver` feature）。启用后，应用进程内会启动一个
W3C WebDriver 协议的 HTTP 服务，监听 `http://127.0.0.1:4445`，通过标准 WebDriver
命令即可驱动应用内的 WKWebView。

> 注意：该服务会暴露完整的自动化能力，**严禁在 release 构建中启用**。
> 默认构建（`npm run tauri dev`、`npm run tauri build`）不包含此插件。

## 启动应用

```bash
# 后台启动（首次编译约需几分钟，之后增量编译约 30 秒）
nohup npm run tauri dev -- --features webdriver > /tmp/anki-marker-tauri-dev.log 2>&1 &

# 轮询等待 WebDriver 服务就绪（"ready":true 即可用）
for i in $(seq 1 60); do
  curl -s -m 2 http://127.0.0.1:4445/status && break; sleep 10;
done
```

## 会话生命周期

```bash
# 1. 创建会话，保存 sessionId
SID=$(curl -s -X POST http://127.0.0.1:4445/session \
  -H 'Content-Type: application/json' \
  -d '{"capabilities":{"alwaysMatch":{}}}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["value"]["sessionId"])')

# 2. ... 执行各种操作 ...

# 3. 结束时删除会话
curl -s -X DELETE http://127.0.0.1:4445/session/$SID
```

关闭应用（会话依赖应用进程，先删会话再关应用）：

```bash
kill $(lsof -ti :4445) 2>/dev/null   # 应用进程（WebDriver 服务）
kill $(lsof -ti :1420) 2>/dev/null   # vite dev server
```

## 常用操作

### 读取窗口标题 / 当前 URL

```bash
curl -s http://127.0.0.1:4445/session/$SID/title   # {"value":"Anki 划词助手"}
curl -s http://127.0.0.1:4445/session/$SID/url     # {"value":"http://localhost:1420/settings"}
```

### 截图（查看 UI 视觉效果）

```bash
curl -s -m 10 http://127.0.0.1:4445/session/$SID/screenshot \
  | python3 -c 'import json,sys,base64; open("/tmp/shot.png","wb").write(base64.b64decode(json.load(sys.stdin)["value"]))'
# 然后用文件读取工具查看 /tmp/shot.png（PNG 图片）
```

元素级截图：`GET /session/{sid}/element/{eid}/screenshot`，返回格式相同。

### 枚举可交互元素（不知道选择器时先做侦察）

```bash
curl -s -X POST http://127.0.0.1:4445/session/$SID/execute/sync \
  -H 'Content-Type: application/json' \
  -d '{"script":"return [...document.querySelectorAll(\"button,a,input,[role=button]\")].map(e=>e.tagName+\" | \"+e.className+\" | \"+e.textContent.trim().slice(0,30))","args":[]}'
```

本应用常见选择器（实测）：
主页 `.setting-button`（设置）、`.header-button`（编辑/粘贴/查询）；
设置页 `.return-button`（返回）、`.reset-button`、`.open-file-button`。

### 点击元素

```bash
# 找元素（返回的 value 是一个对象，取其第一个值作为元素 id）
EID=$(curl -s -X POST http://127.0.0.1:4445/session/$SID/element \
  -H 'Content-Type: application/json' \
  -d '{"using":"css selector","value":".setting-button"}' \
  | python3 -c 'import json,sys; print(list(json.load(sys.stdin)["value"].values())[0])')

# 点击
curl -s -X POST http://127.0.0.1:4445/session/$SID/element/$EID/click \
  -H 'Content-Type: application/json' -d '{}'
```

### 输入文本

```bash
curl -s -X POST http://127.0.0.1:4445/session/$SID/element/$EID/value \
  -H 'Content-Type: application/json' -d '{"text":"hello"}'
```

### 滚动（W3C Actions 滚轮事件）

```bash
curl -s -X POST http://127.0.0.1:4445/session/$SID/actions \
  -H 'Content-Type: application/json' \
  -d '{"actions":[{"type":"wheel","id":"w1","actions":[{"type":"scroll","x":400,"y":250,"deltaX":0,"deltaY":600}]}]}'
curl -s -X DELETE http://127.0.0.1:4445/session/$SID/actions   # 释放输入状态
```

### 执行任意 JavaScript

```bash
# 同步
curl -s -X POST http://127.0.0.1:4445/session/$SID/execute/sync \
  -H 'Content-Type: application/json' \
  -d '{"script":"return document.title","args":[]}'

# 异步（script 末尾通过回调返回）：参数为注入的回调函数
curl -s -X POST http://127.0.0.1:4445/session/$SID/execute/async \
  -H 'Content-Type: application/json' \
  -d '{"script":"const done=arguments[arguments.length-1]; setTimeout(()=>done(\"ok\"),500);","args":[]}'
```

## 关键注意事项（实测踩坑记录）

1. **应用窗口必须可见、不被其他窗口遮挡**。
   WKWebView 在窗口被遮挡时会暂停渲染（macOS App Nap）：截图拿到的是旧帧，
   CSS 过渡动画也会延迟到窗口重新可见时才播放。如果发现“DOM/URL 已变化但截图没变化”，
   先怀疑窗口被遮挡。
2. **不要中断进行中的 HTTP 请求**。
   插件串行处理命令，请求被中途取消会导致响应流错位一条（下一条请求拿到上一条的响应）。
   错位后再发一条轻量请求（如 `GET /url`）即可重新对齐；严重时重建会话。
   建议所有 `curl` 加 `-m` 超时而不是手动取消。
3. **点击/导航后给渲染留时间**。路由跳转有过渡动画，`sleep 1~2` 后再截图。
4. **截图前可先确认 DOM 状态**（用 `execute/sync` 读 `location.pathname`、元素文本等），
   截图与 DOM 互相印证，避免被旧帧误导。
5. **JavaScript 控制台消息/报错直接读日志，不需要 devtools**。
   应用已通过 `tauri-plugin-log` 把前端的 `console.log/warn/error/...`、未捕获异常
   （`window.onerror`）、未处理的 Promise 拒绝（`unhandledrejection`）和 Vue 组件错误
   （`app.config.errorHandler`）全部转发到 Rust 侧统一日志（见 `src/logics/logging.ts`），
   转发记录带 `[webview]` 标签。查看方式：
   - dev 终端输出：`grep -a '\[webview\]' /tmp/anki-marker-tauri-dev.log | tail -50`
     （即启动时 nohup 重定向的那个文件）
   - 日志文件（release 同样写入）：`~/Library/Logs/com.zhb2000.anki-marker/Anki 划词助手.log`
   - 注意 WebKit 的 `error.stack` 不含错误消息文本，`logging.ts` 已做拼接处理。
   - 相同消息在 1 秒窗口内会被去重，刷屏停止后输出一条
     `[repeated N times, duplicates suppressed] ...` 汇总（类似 devtools 的重复计数）；
     Vue 组件错误会附带 `[component] App > ... > 出错组件` 组件链路行。
6. **sendKeys 不遵守 disabled 属性（实测）**。
   向 `disabled` 的 input 发 `/value` 命令返回成功且值被写入，并会触发 Vue v-model
   → 自动保存真实配置文件。探针禁用态控件时请改用 DOM 断言（`el.disabled === true`）
   或点击类操作（原生 disabled 按钮的 click 会被正常抑制）；若已污染配置，用
   `el.value=...; el.dispatchEvent(new Event("input",{bubbles:true}))` 写回并核对 config.toml。

## 端到端流程示例

```bash
nohup npm run tauri dev -- --features webdriver > /tmp/dev.log 2>&1 &
for i in $(seq 1 60); do curl -s -m 2 http://127.0.0.1:4445/status && break; sleep 10; done

SID=$(curl -s -X POST http://127.0.0.1:4445/session -H 'Content-Type: application/json' \
  -d '{"capabilities":{"alwaysMatch":{}}}' \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["value"]["sessionId"])')

# 点击设置按钮 → 截图设置页 → 滚动 → 点击返回 → 截图主页
EID=$(curl -s -X POST http://127.0.0.1:4445/session/$SID/element -H 'Content-Type: application/json' \
  -d '{"using":"css selector","value":".setting-button"}' \
  | python3 -c 'import json,sys; print(list(json.load(sys.stdin)["value"].values())[0])')
curl -s -X POST http://127.0.0.1:4445/session/$SID/element/$EID/click -H 'Content-Type: application/json' -d '{}'
sleep 2
curl -s http://127.0.0.1:4445/session/$SID/screenshot \
  | python3 -c 'import json,sys,base64; open("/tmp/settings.png","wb").write(base64.b64decode(json.load(sys.stdin)["value"]))'

curl -s -X DELETE http://127.0.0.1:4445/session/$SID
kill $(lsof -ti :4445) 2>/dev/null; kill $(lsof -ti :1420) 2>/dev/null
```
