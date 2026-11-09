# 开发者功能（dev 功能）

目标：让开发者，尤其是 AI，在不用人在窗口里点点点、不用人复制日志的前提下，复现各种界面状态、使用应用的**真实功能**，
并自己看到“刚才发生了什么”。

## 总开关与入口

唯一的 Vite 环境变量是 `VITE_DEV_FEATURES`（`.env.development` 默认 true）。关闭时：设置里没有“开发者”页、没有 `window.__dev`、
没有浏览器模式，`src/dev` 不会进入产物。任何构建（含 release）里它恒为关闭（`vite.config.ts` 注入编译期常量 `__DEV_FEATURES__`）。

| 入口 | 适用者 | 说明 |
|---|---|---|
| **AI 推荐：真实 Rust + 浏览器** | AI | `npm run dev:ai`（放后台跑），就绪后输出 `[dev-ai] READY`。以 `dev-bridge` feature 启动 Tauri dev，用浏览器工具打开 `http://localhost:1420/`（选哪个见下文“选浏览器工具与内核”），页面自动连到真实 Rust。`node scripts/dev-ai.mjs status` 查询是否就绪 |
| 仅夹具（不编译 Rust） | AI | `npm run dev:browser`，端口 1421（`DEV_PORT` 可改），全部命令由夹具应答，适合快速看界面状态与 AI 场景 |
| 真机窗口 | 人类 | `npm run tauri dev`。人类可以直接操作原生 GUI；AI 场景也在这条路径生效（LLM 默认跟随环境真实请求，免 Key 预览需打开“免 Key 预览 AI”），见下文“AI 场景” |
| 自动化操作真机 | AI（必要时） | `npm run tauri dev -- --features webdriver`，受窗口遮挡影响，见 `automation-debugging.md` |

## 选浏览器工具与内核

**优先用 `playwright-cli`**：它能指定内核，渲染与事件行为和真机对得上；`agent-browser`（vercel-labs）只有 Chromium，适合临时看页面、抓文本，内核相关的问题不要用它下结论。

内核按平台选，与 Tauri 在真机上用的保持一致：

| 平台 | 真机内核 | 用法 |
|---|---|---|
| macOS | WKWebView | `playwright-cli open --browser=webkit <url>` |
| Linux | WebKitGTK | `playwright-cli open --browser=webkit <url>` |
| Windows | WebView2（Chromium） | `playwright-cli open --browser=msedge <url>`（或 `chrome`） |

- 界面与手势问题先在对应内核复现：macOS/Linux 的问题在 Chromium 里复现不出来是正常的，反之亦然（如拖到窗口外松开时 mouseup 丢不丢，各内核就不一致）。
- 需要逐事件构造手势（按下、多次移动、抬起分开下发）或中途 `eval` 读组件内部状态时，只有 playwright-cli 够用；它的会话是常驻的，两条命令之间不会丢页面。
- 浏览器版本与 CLI 绑定，首次 `open --browser=webkit` 报缺内核时按提示安装（`npx playwright install webkit`）。
- 系统取词、托盘、全局快捷键、原生窗口在浏览器里测不了，仍要真机复核（见 `automation-debugging.md`）。

## 桥接（dev-bridge）：浏览器里使用真实功能

Rust 侧 `src-tauri/src/application/dev_bridge.rs`（Cargo feature `dev-bridge`，release 构建会 `compile_error!`）起一个本机 HTTP 服务：

- invoke 通过 Tauri 公开的 `WebviewWindow::on_message` 转给主窗口，走 `generate_handler!` 里**全部已注册命令**，
  含 State 注入、权限检查和插件命令——**新增命令、新增事件无需改任何桥接代码**。
- 前端 `listen` 某个事件时，服务端用 `listen_any` 订阅并经 SSE 推给浏览器（真实的 `config-changed`、`sentence-captured` 等都能收到）。
- 浏览器经 Vite 的 `/__dev/bridge/*` 代理访问（令牌只在代理里注入，浏览器拿不到；要求本机 Host 与自定义头 `X-Dev-Bridge`）。
  端口与令牌写在 `dev-bridge.local.json`（gitignore）。

桥接下**仍由夹具处理**的命令（有真实副作用）：打开程序/目录、全局快捷键挂起恢复、开机自启动、剪贴板、原生窗口、
**AnkiConnect 请求与 Anki 的启动探测**（默认模拟，不会写进你真实的牌组；“Anki 状态”选“真实 Anki”才连真的）。
配置读取是真实的，但**写入默认只落在浏览器会话**（“桥接下配置的写入位置”），不会改你本机的开发配置和 API Key。

**AI 请求按环境取默认值：Tauri 应用默认真实请求，浏览器桥接/夹具默认模拟。** “跟随环境”是 LLM 场景的默认值；显式切到“真实请求”才会在所有环境联网，显式选任一模拟场景才会在所有环境用夹具。
拦截点在业务侧的 HTTP 入口（`src/logics/net.ts`），不在 Tauri IPC 层，所以三种入口共用同一套规则。

注意：原生窗口同时在跑同一份前端，划词事件等会被两边各收到一次；但 AI 场景在原生窗口与浏览器页都生效，不会出现“浏览器里是 mock、原生窗口在烧 token”的割裂。

## AI 场景（LLM / AI 优选）

AI 相关请求由 `src/dev/mock/llm.ts` 处理。`llmScenario` 默认 `auto`：Tauri 应用默认真实请求，浏览器（桥接/夹具）默认模拟；显式选择可以覆盖环境默认。覆盖：

- AI 优选：命中候选、fallback、整句翻译、拼写错误、乱码；
- 思考模型的思维链（reasoning）与流式输出；
- 参数降级重试（400 → json_object）与编号错误后的纠正重试；
- HTTP 错误、空响应、非法 JSON；
- 设置页“测试连接”与“获取模型列表”。

默认值 `llmScenario='auto'`：Tauri 联网、浏览器不联网。要强制真实请求请切到“真实请求”；要强制模拟请选任一模拟场景。

### 零配置预览 AI

打开“免 Key 预览 AI”（`mockLlmConfig`）后：

- `read_config` 会返回 `http://dev-mock.local/v1`、假 Key、假模型，并启用 AI；
- **不修改真实配置文件**；
- 模拟配置生效期间，`commit_config` 会剔除 LLM 相关键，避免假配置写进真实配置；
- 关闭开关后恢复真实配置；
- 只在非“真实请求”场景生效；
- 在 `auto` 下开启它会**强制使用模拟场景**（否则 Tauri 的默认真实请求不会使用假配置）；如果 LLM 场景已显式选“真实请求”，此开关会被禁用并提示无效。

设置页“AI 优选”顶部会出现“开发模拟”警示卡。这个开关在 `tauri dev`、`dev:ai`、`dev:browser` 都有效，所以没有 API Key 也能预览 AI。

### 场景与预期

| `llmScenario` | 预期界面/行为 |
|---|---|
| `auto` | Tauri 真实请求；浏览器桥接/夹具按 `pick` 模拟（默认） |
| `pick` | AI 卡命中词典条目，显示“笔记：（dev mock）已选中第一条候选” |
| `thinking` | 先出现“AI 正在思考”，随后进入正文 |
| `fallback` | 显示“AI 生成 / 非词典条目”和模拟语境释义 |
| `sentence` | 显示“AI 译文 / 整句”和模拟整句译文 |
| `misspelling` | 显示“未收录”和“（dev mock）正确拼写：design” |
| `gibberish` | 显示“未收录”和“（dev mock）无法识别的乱码或误触” |
| `bad-json` | AI 卡进入失败态，错误为 `no JSON object found in AI output` |
| `http-error` | AI 卡进入失败态，错误包含 `HTTP 401` |
| `empty` | AI 卡进入失败态，错误为 `stream ended with empty content` |
| `compat-retry` | 第一次请求 400，`warnings` 出现“参数不被接受，降级后重试”，随后成功 |
| `bad-pick-retry` | 第一次给非法编号，`warnings` 出现“追加提醒后重试一次”，第二次成功 |
| `real` | 真实请求；夹具模式（`dev:browser`）没有 Rust 网络，会失败 |

思考态是瞬态：默认每块 120ms，想截图或看清可以调大到 150–300ms；`idle()` 等的是“安静”，抓瞬态文案用 `waitFor` 或短间隔轮询 `__dev.text()`。思维链内容不进 UI，这是产品设计，夹具只呈现“AI 正在思考”的状态。

切换场景会自动清空 AI 结果缓存与参数兼容缓存；已经显示在界面上的卡片需要点“重新生成”或重新查询才会按新场景重走。也可以随时点开发者页的“重置模拟状态”或调用 `__dev.resetMock()`。

### 直接复制的最小配方

```bash
# 命中候选（零配置）
agent-browser open 'http://localhost:1421/?dev.llmScenario=pick&dev.mockLlmConfig=true'

# 思考模型 + 慢速流式（观察“AI 正在思考”和打字机）
agent-browser open 'http://localhost:1421/?dev.llmScenario=thinking&dev.llmStreamDelayMs=150&dev.mockLlmConfig=true'

# 失败态
agent-browser open 'http://localhost:1421/?dev.llmScenario=http-error&dev.mockLlmConfig=true'
```

```bash
agent-browser eval '(async () => {
  await __dev.idle();
  const m = __dev.mark();
  __dev.fill("回车查询单词", "design");
  __dev.press("Enter", "回车查询单词");
  await __dev.waitFor("重新生成", { timeoutMs: 5000 });
  await __dev.idle({ quietMs: 400 });
  return { text: __dev.text(), report: await __dev.report(m) };
})()'
```

`report().invokes` 里会看到 `http POST dev-mock.local/v1/chat/completions [dev pick] 200 0ms` 这样的记录；
`idle()` 会把夹具的流式输出算进“尚未安静”，不会在正文还没写完时提前返回。

真实请求：

```bash
# 只在 dev:ai / tauri dev 下有真实网络；会使用配置中的真实 API Key，产生费用
agent-browser open 'http://localhost:1421/?dev.llmScenario=real&dev.mockLlmConfig=false'
```

## 给 AI 的调试闭环

页面里的 `window.__dev.help()` 会打印用法。核心套路：**把一整段操作写进同一条 eval**——用 agent-browser 时标签页在两条命令之间可能回到空白页，
元素编号每次快照都会变；playwright-cli 的会话常驻，但也建议一条 eval 走完一段操作，少来回、输出更完整。
`__dev` 的助手按可见文字定位元素、用 `idle()` 等应用安静，不需要编号和 sleep。
eval 里**直接返回对象**（不要 `JSON.stringify`），输出才不会被二次转义。

```bash
agent-browser open 'http://localhost:1420/?dev.ankiState=plugin-missing'    # URL 参数：只对本页生效，不写任何文件
agent-browser eval '(async () => {
  await __dev.idle();
  const m = __dev.mark();                                  // 1. 取游标
  __dev.fill("回车查询单词", "serendipity");               // 2. 操作（按可见文字）
  __dev.press("Enter", "回车查询单词");
  await __dev.idle({ quietMs: 600 });
  __dev.click("添加到 Anki");
  await __dev.idle({ quietMs: 600 });
  return await __dev.report(m);                            // 3. 一次拿回发生的一切
})()'
```

`report` 返回：`errors`（未捕获异常、console.error）、`failedInvokes`（失败的命令及错误）、`warnings`、
`invokes`（命令与 HTTP 调用轨迹，如 `http POST localhost:8765/ [addNote] 200 81ms`、`search_collins 21ms`）、
`dialogs` / `alerts`（页面上弹出的对话框与提示的**文字**，不用截图）、`mocks`（当前环境实际生效的模拟清单）、`rustRecentWarnings`（Rust 日志里近期 WARN/ERROR）。

| 接口 | 作用 |
|---|---|
| `__dev.mark()` / `await __dev.report(since)` | 游标与“自游标以来”的汇总 |
| `__dev.click(文字或选择器, nth?)` | 点击；按可见文字 / aria-label / title 匹配。找不到或有多个同名元素时报错并列出候选（含所在区块的开头文字），用 `nth`（从 0 起，文档顺序）指定第几个 |
| `__dev.fill(placeholder或标签, 值, nth?)` / `__dev.press(键, 目标?)` | 填输入框（触发 v-model）/ 按键 |
| `await __dev.idle({quietMs, timeoutMs})` | 等到无进行中的命令/请求且 DOM 静止；超时不抛错，返回 `settled: false`。**看不到定时器**：应用在做重试/宽限期这类 `setTimeout` 式等待时（如 Anki 连不上时约 8 秒后才弹错误），它会误判“已安静” |
| `await __dev.waitFor(文字或正则, {timeoutMs})` | 等页面可见文字里出现某段文字，如错误对话框的标题；超时返回 `found: false`。有重试/宽限期的流程用它代替 `idle` |
| `__dev.text(max?)` | 页面可见文字 |
| `__dev.logs({since, level, kind})` | 原始条目（kind: console / error / invoke） |
| `await __dev.rustLogs({lines, grep, trace})` | Rust 日志文件尾部（含前端 console 转发进来的内容；默认滤掉 TRACE） |
| `__dev.list()` | 发现全部场景、可选值与当前来源 |
| `__dev.get()` | 无参；返回当前全部生效值 |
| `__dev.set(k, v)` / `reset(k?)` | 修改/清除会话覆盖；仅当前页面有效，刷新后失效 |
| `__dev.resetMock()` | 清空 AI 结果缓存与参数兼容缓存，让下一次请求按当前场景重走 |
| `__dev.mockStatus()` | 当前环境、所有实际生效的模拟、无效值警告与禁用项；`report().mocks` 是同一份数据 |
| `__dev.emit(event, payload)` | 浏览器里模拟后端事件，`__dev.sampleCapture` 是一份划词载荷示例 |
| `__dev.backend` | `'bridge'`（真实 Rust）或 `'fixtures'` |

### 先看清“现在有哪些模拟”

开发者页顶部有一条醒目的“当前有 N 项模拟生效”横幅；同样的数据可以通过 `__dev.mockStatus()` 拿到，`report()` 也会带上它：

```js
{
  environment: 'browser-bridge',
  environmentLabel: '浏览器 + 桥接',
  mocks: [
    { key: 'llmScenario', title: 'AI / LLM', value: '命中候选', detail: '...' },
    { key: 'bridgeConfigWrites', title: '配置写入', value: '仅浏览器会话', detail: '...' },
  ],
  warnings: [{ key: 'llmScenario', message: '...' }],
  disabled: [{ key: 'platform', reason: '...' }],
  effective: { llmScenario: 'pick' },
  settings: { /* 每个 key 的 enabled / disabledReason / warning / effectiveValue */ },
}
```

**在开始调试前先调一次 `__dev.mockStatus()`**：它可以回答“当前是不是夹具、AI 会不会联网、哪些设置其实没生效、当前环境禁用哪些项”。
页面上的禁用与警示色描述也来自同一份计算（`src/dev/status.ts`），不会出现 UI 说一套、接口说一套。

设置页的规则：

- 当前模式下完全无效的项会禁用并写明原因（如 Tauri 下的平台模拟、Anki 状态、浏览器后端、配置写入位置）；
- 当前值无效的项用 WinUI warning 色（浅色 `#9a6700`、深色 `#d4a72c`）提示描述文字，卡片边框保持中性（如夹具模式选“真实 Anki/真实请求”、显式真实请求时开“免 Key 预览 AI”、没有模拟请求时调模拟延迟）。

截图用浏览器工具的 `screenshot`；能用 `report` / `text` 回答的问题优先用文字，省 token 也更准。
不开浏览器也能看 Rust 日志：`curl -H 'X-Dev-Bridge: 1' 'http://localhost:1420/__dev/bridge/logs?lines=100&grep=error'`。
启动/停止：`npm run dev:ai`（后台跑）、`node scripts/dev-ai.mjs status`、`node scripts/dev-ai.mjs stop`。

## 多个 worktree / 多个 AI 并行开发

端口与实例标识由环境变量决定（`scripts/dev-env.mjs`，vite、tauri 包装器、`dev-ai` 共用），**严格模式：端口被占用就失败，绝不自动顺延**。

| 用法 | 行为 |
|---|---|
| 什么都不设 | 与以往完全一致：端口 1420、共用 identifier（沿用已授权的辅助功能与已有配置） |
| `DEV_PORT=1430 npm run dev:browser` | 只换 Vite 端口，夹具浏览器模式（不起 Rust），可与其他实例并行；缺省 1421 |
| `DEV_INSTANCE=foo DEV_PORT=1430 npm run dev:ai` | 并行跑完整的 Tauri：identifier / 产品名带 `.foo` 后缀，单实例互斥、配置目录、日志文件全部独立 |

- 设了 `DEV_INSTANCE` 就必须显式给 `DEV_PORT`，否则直接失败（避免默认撞上日常实例的 1420）。
- 只换端口、不设实例名，并且已有同 identifier 的 Tauri 在运行时，单实例插件会让新进程**悄悄退出**；`dev-ai` 的启动前检查与“就绪前退出”提示会指出这一点。
- `dev-ai` 启动前先探测端口：被占用时直接报错并说明占用者是哪个 worktree（是不是自己），退出码 3。`status` 的 JSON 里 `own` 表示端口上的是不是本 worktree，AI 以它判断“就绪的是不是自己的实例”。
- 隔离实例的配置从模板新建，没有日常实例里的 API Key 和全局快捷键：不会多个 AI 同时烧 token，也不会争抢快捷键；辅助功能授权按 identifier 记，隔离实例需重新授权（并行实例走桥接，划词取词本就不在浏览器里测）。
- 桥接端口随机分配，连接信息写在各 worktree 自己的 `dev-bridge.local.json`；`dev-settings.local.json`、`dev-ai.local.pid` 同样按 worktree 隔离。每个 worktree 各自编译 Rust（各自的 `target/`）。
- WebDriver（固定 4445）不支持并行：并行开发请用桥接，不要用 WebDriver。

## 开发者设置

注册表在 `src/dev/schema.ts`（标题、说明、默认值、取值范围），设置页“开发者”入口、文件校验、URL 参数和 `window.__dev` 都由它生成。
设置页按语义分组：**AI 与 LLM、Anki 与模板、更新与版本、浏览器模式、其他行为**；完全无效的项会禁用并写明原因，值无效的项会用 WinUI warning 色提示。
新增一项：改注册表；业务代码经 `logics/debug.ts` 读取，纯 dev 夹具可直接读 `src/dev/state.ts` 的 `devSettings`。
取值优先级：URL 参数 / `window.__dev.set`（仅当前页面） > `dev-settings.local.json`（项目根，gitignore，
设置页修改立即生效并写入，外部编辑也会实时同步；读写经 Vite 插件 `/__dev/settings`）> 默认值。

默认值原则：不产生外部副作用的读取/展示尽量与真实行为一致；有副作用的外部调用由显式场景控制——Anki 写入默认模拟，AI 按环境默认（Tauri 真实、浏览器模拟）。
需要按环境取不同默认值的设置用 `auto` 在运行时解析，**不拆成两个键**：目前只有 LLM 场景。
拆键会让同一个行为在两个入口各有一份值，容易漂移，也让 URL 参数/文件配置的优先级更难解释。

## 约束与局限

- 应用模块顶层不要访问 Tauri 运行时 API（如 `os.type()`），用函数惰性读取——浏览器模式的 IPC 打桩在应用模块之前才装好。
- **真实 Tauri 命令的调用轨迹只在浏览器模式可用**：真实 Tauri 窗口里 `__TAURI_INTERNALS__.invoke` 是不可写、不可配置的属性，包装会抛 TypeError。原生窗口里 `report().invokes` 仍会包含 dev 夹具直接处理的 HTTP 请求（由拦截器补记），但不包含真实 Tauri 命令（console / 报错 / 对话框文字照常）。dev 初始化的任何失败都不会阻塞应用挂载。
- 浏览器内核要与平台对齐（macOS/Linux 用 `webkit`、Windows 用 Chromium，见“选浏览器工具与内核”）；即便如此，系统取词、托盘、全局快捷键在浏览器里仍测不了。真机复核见 `automation-debugging.md`（含截图前的可见性保护）。
- `on_message` 在 Tauri 文档里标注为非稳定 API（供自定义 invoke 系统使用）；升级 Tauri 后若桥接编译失败，只需修 `dev_bridge.rs` 一个文件。
- playwright-cli / agent-browser 的浏览器若在沙箱里启动失败（`sandbox initialization failed`），需在沙箱外运行。

## 卡片模板的预览与构建

笔记模板不是本应用的一部分：它是一段经 AnkiConnect 发给 Anki 的自包含字符串。源码在 `src/card-template/`，
由 `scripts/card-template.mjs` 在 Vite 的 `load` 钩子里按需构建、改源码时失效缓存并整页重载。
`tauri dev`、`dev:ai`、`dev:browser`、`vitest` 与 `vite build` 走的是同一段代码，**没有单独的构建步骤**。

| 文件 | 作用 |
|---|---|
| `front.html` / `back.html` | mustache 模板；`<!-- card-script -->` 处插入打包后的脚本 |
| `script.ts` | 卡片脚本，打包成单个 IIFE（换卡时脚本会重复执行，靠这层保证安全） |
| `style.css` | 样式；图标以 `url('./icons/x.png')` 引用，构建时内联为 data URL |
| `icons/` | 图标源文件（PNG / SVG） |
| `release-note.md` | 模板更新说明，设置页展示 |

预览页：设置 → 开发者 → “卡片模板预览”，或直接访问 `/dev/card`。渲染的就是构建产物（与发给 Anki 的完全一致），
可切样本、预览宽度与夜间模式；链接点击会被拦下并显示目标地址，折叠与朗读按原样执行。

依赖约束：`src/shared/` 与 `src/card-template/` 只能用标准 Web API。构建期有白名单（引入应用运行时直接构建失败）、
lint 有限制导入、产物测试会连续执行两次脚本。要共用逻辑请放进 `src/shared/`。
