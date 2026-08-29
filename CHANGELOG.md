# 更新日志
## [0.2.1] - 🚧 In Progress

为更多平台构建。

### 划词助手更新内容

- 新增 macOS“关闭窗口行为”设置：可选择关闭窗口后应用是否**保持后台运行**（默认保持，关闭后仅隐藏窗口，可由 Dock 图标、菜单栏图标或全局快捷键再次唤起；关闭此项则点关闭按钮直接退出应用）；保持运行时可进一步选择后台运行期间应用图标的显示位置——**Dock 栏**、**菜单栏**（屏幕右上角）或**都不显示**。窗口打开时图标始终显示在 Dock 栏，仅在后台运行期间按此选项显示。
- 新增**自动启动 Anki** 功能：添加卡片时若检测到 Anki 未运行，将自动启动 Anki（Windows 下最小化启动）并等待 AnkiConnect 就绪，无需再手动打开 Anki。相关行为可在设置中调整：“自动启动 Anki”、“应用启动时启动 Anki”、“Anki 可执行文件路径”。
- 若 Anki 已在运行但无法连接 AnkiConnect，添加卡片时会提示安装/启用 AnkiConnect 插件，而非无谓等待。
- 编辑句子时，已选中的单词不再轻易丢失选中状态：编辑框内容视为草稿，确认编辑后才重建分词（拆开再复原的单词可保留选中）；编辑中被改写的单词（如 fox→foxes）也会尽力把选中状态转移到新单词上。

添加卡片时，划词助手会自动检测与 AnkiConnect 的连接：如果检测到 **Anki 没有在运行**，划词助手将**自动启动 Anki** 并等待其就绪，通常无需再手动打开 Anki（macOS 下 Anki 在后台启动且不抢占前台，Windows 下最小化启动）。你可以在设置中关闭“自动启动 Anki”，或开启“应用启动时启动 Anki”；如果 Anki 安装在非默认位置，请在设置中手动指定“Anki 可执行文件路径”。

> [!NOTE]
> macOS 下如需让自动启动的 Anki **完全隐藏**（连窗口都不闪现），请在“系统设置 → 隐私与安全性 → 辅助功能”中为划词助手开启权限；未授权时 Anki 会以不抢占前台的方式启动。

> [!TIP]
> 推荐为 Anki 安装 [Minimize to tray](https://ankiweb.net/shared/info/85158043)（最小化到托盘）插件：关闭 Anki 窗口后 Anki 将常驻系统托盘，AnkiConnect 服务始终保持在线，添加卡片时无需再等待 Anki 启动。

### 单词笔记模板更新内容

本次更新将划词助手的单词笔记模板升级至 0.3.0 版本，模板的更新内容如下：

- 卡片模板适配 Anki 深色（夜间）模式。

## [0.2.0] - 2026-01-01

各位朋友，新年好！🎉划词助手再次迎来了久违的更新。

### 划词助手更新内容

- 新增查询**有道在线词典**功能。
- 完善单词释义列表记忆滚动位置的功能。
- 应用框架升级至 Tauri v2。

### 单词笔记模板更新内容

本次更新将划词助手的单词笔记模板升级至 0.2.0 版本，模板的更新内容如下：

- 更新 API 以**修复例句朗读失效**的问题。
- 卡片切换后暂停例句朗读。

<details open>
<summary><strong>点击展开/收起模板升级提示</strong></summary>
<br>

如果你使用过本应用的旧版本或 [mmjang](https://github.com/mmjang) 开发的 Android 版（[mmjang / ankihelper](https://github.com/mmjang/ankihelper)），那么需要手动确认是否**将已有的旧模板升级为新模板**：

1. 打开 Anki 软件（事先安装好 AnkiConnect 插件）。
1. 点击划词助手右上角的设置按钮，进入设置页面。
1. 点击设置页面的“更新模板”按钮，并在弹出的对话框中确认更新。

建议更新模板，以在复习单词时获得更好的体验。
</details>

<details open>
<summary><strong>点击展开/收起下载链接</strong></summary>
<br>

**Windows**

- **x86-64:**
   - [Installer (User) (.exe)](https://github.com/zhb2000/anki-marker/releases/download/v0.2.0/anki-marker_0.2.0_windows_x64-setup.exe)
   - [Installer (System) (.msi)](https://github.com/zhb2000/anki-marker/releases/download/v0.2.0/anki-marker_0.2.0_windows_x64.msi)
   - [Portable (.zip)](https://github.com/zhb2000/anki-marker/releases/download/v0.2.0/anki-marker_0.2.0_windows_x64-portable.zip)
</details>

## [0.1.0] - 2025-01-02

各位朋友，新年快乐！🎉

### 划词助手更新内容

- 支持**编辑单词笔记**。单词添加成功后，点击单词条目右侧的“编辑”按钮，即可打开 Anki 的卡片编辑器，对所添加的单词笔记进行编辑。
- 新增应用和模板的检查更新功能。
- 修复在点击配置文件的“打开目录”时出现 cmd 窗口的问题。
- 修复在配置文件路径包含空格时“打开目录”可能失败的问题。

### 单词笔记模板更新内容

本次更新将划词助手的单词笔记模板升级至 0.1.0 版本，模板的更新内容如下：

- 新增**例句朗读**功能，点击例句中的喇叭图标可以朗读例句，再次点击可以停止朗读。例句朗读的语音来自有道词典，需要联网使用。
- 为条目的**展开和收起**添加了**动画效果**，点击条目的标题可以展开或收起条目的内容。
- 其他卡片布局优化。

<details>
<summary><strong>点击展开/收起模板升级提示</strong></summary>
<br>

如果你使用过本应用的 0.0.1 版本或 [mmjang](https://github.com/mmjang) 开发的 Android 版（[mmjang / ankihelper](https://github.com/mmjang/ankihelper)），那么需要手动确认是否**将已有的旧模板升级为新模板**：

1. 打开 Anki 软件（事先安装好 AnkiConnect 插件）。
1. 点击划词助手右上角的设置按钮，进入设置页面。
1. 点击设置页面的“更新模板”按钮，并在弹出的对话框中确认更新。

你也可以继续使用旧模板，这不会影响划词助手的使用。但是，建议你更新模板，以在复习单词时获得更好的体验。

如果你的 Anki 软件中没有划词助手的旧模板，或者你是第一次使用划词助手，那么无需进行上述操作，直接使用即可。
</details>

<details>
<summary><strong>点击展开/收起下载链接</strong></summary>
<br>

**Windows**

- **x86-64:**
   - [Installer (User) (.exe)](https://github.com/zhb2000/anki-marker/releases/download/v0.1.0/anki-marker_0.1.0_windows_x64-setup.exe)
   - [Installer (System) (.msi)](https://github.com/zhb2000/anki-marker/releases/download/v0.1.0/anki-marker_0.1.0_windows_x64.msi)
   - [Portable (.zip)](https://github.com/zhb2000/anki-marker/releases/download/v0.1.0/anki-marker_0.1.0_windows_x64-portable.zip)
</details>

## [0.0.1] - 2024-03-17

第一个版本。

<details>
<summary><strong>点击展开/收起下载链接</strong></summary>
<br>

**Windows**

- **x86-64:**
   - [Installer (User) (.exe)](https://github.com/zhb2000/anki-marker/releases/download/v0.0.1/anki-marker_0.0.1_x64-setup.exe)
   - [Installer (System) (.msi)](https://github.com/zhb2000/anki-marker/releases/download/v0.0.1/anki-marker_0.0.1_x64.msi)
   - [Portable (.zip)](https://github.com/zhb2000/anki-marker/releases/download/v0.0.1/anki-marker_0.0.1_windows-x64-portable.zip)
</details>
