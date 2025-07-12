<div align="center">
<img src="./src-tauri/icons/icon.png" alt="Anki 划词助手" title="Anki 划词助手" width="64">
</div>

<div align="center"><h1>Anki 划词助手</h1></div>

Anki 划词助手是一个制作 Anki 卡片的工具，你可以用划词助手标记句子中的生词，通过“**单词结合上下文**”的方式更好地背单词。

本项目受到了 [mmjang / ankihelper](https://github.com/mmjang/ankihelper) 的启发。由于原项目是 Android 应用，且已经不再维护，而我自己用电脑的时间更多，因此自己用 [Tauri](https://github.com/tauri-apps/tauri) 写了一个类似的工具。

<p align="center">
<img src="./docs/assets/main-screenshot.png" width="600" alt="主界面" title="主界面">
</p>

# 安装
## 安装划词助手本体

[Releases 页面](https://github.com/zhb2000/anki-marker/releases)提供了 Windows 平台的安装程序（.msi/.exe）和便携式应用（.zip），其余平台请自行编译。

Anki 划词助手是一个基于 Tauri 的桌面应用，你的 Windows 系统需要带有 [Microsoft Edge WebView2](https://developer.microsoft.com/zh-cn/microsoft-edge/webview2/) 才能运行（Windows 10 2004 及以上版本已经自带）。

## 安装辅助工具

Anki 划词助手需要通过 AnkiConnect 插件与 Anki 进行通信，你需要先安装 **Anki 应用**和 **AnkiConnect 插件**：

1. 安装 Anki 应用：[Anki - powerful, intelligent flashcards](https://apps.ankiweb.net/)。
1. 安装 AnkiConnect 插件：[AnkiConnect - AnkiWeb](https://ankiweb.net/shared/info/2055492159)。安装方法如下：
    1. 打开 Anki，点击“工具-插件”。
    1. 点击“获取插件”，输入 AnkiConnect 的代码 `2055492159`，点击“确定”。
    1. 重启 Anki 应用。

# 使用

使用 Anki 划词助手时，请确保 **Anki 应用已经打开**，AnkiConnect 的服务会在 Anki 启动时自动开启。

AnkiConnect 默认会在 `localhost:8765` 上启动一个 HTTP 服务，如果你修改了 AnkiConnect 的端口号，请在设置中将“AnkiConnect 服务”这一项修改为对应的 URL：

<p align="center">
<img src="./docs/assets/settings-screenshot.png" width="600" alt="应用设置" title="应用设置">
</p>

划词界面：

<p align="center">
<img src="./docs/assets/main-screenshot.png" width="600" alt="主界面" title="主界面">
</p>

添加的 Anki 卡片（正面/背面）：

<p align="center">
<img src="./docs/assets/anki-card-front.png" alt="Anki 卡片正面" title="Anki 卡片正面" width="300">
<img src="./docs/assets/anki-card-back.png" alt="Anki 卡片背面" title="Anki 卡片背面" width="300">
</p>

# 克隆本仓库

本项目使用 Git LFS 管理资源文件，请在克隆前先安装 Git LFS，否则部分资源文件无法正常下载。安装教程参见 [Git LFS 官网](https://git-lfs.com/)。

Git LFS 安装成功后，像平常的 Git 仓库那样运行 `git clone` 命令即可。

# 开发

在项目的根目录下运行 `npm install` 安装依赖：

```shell
npm install
```

`tauri info` 检查开发环境是否满足要求：

```shell
npm run tauri info
```

`tauri dev` 启动开发模式（支持热重载）：

```shell
npm run tauri dev
```

`tauri build` 构建应用：

```shell
npm run tauri build
```

打包好的应用位于 `src-tauri/target/release/bundle` 目录下。

在 Windows 平台上可以使用 `build-for-windows.js` 这个脚本，同时将应用打包成安装程序（.msi/.exe）和便携式应用（.zip）：

```shell
node build-for-windows.js
```

打包好的安装程序和便携式应用位于 `src-tauri/target/release/release-assets` 目录下。
