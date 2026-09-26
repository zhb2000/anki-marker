use std::path::Path;

/// 后台运行（关闭窗口保持运行）期间应用图标的显示位置
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackgroundIcon {
    /// Dock 栏图标（仅 macOS；其他平台按菜单栏/托盘图标处理）
    Dock,
    /// 屏幕顶部菜单栏图标（macOS）/ 系统托盘图标（Windows/Linux）
    MenuBar,
    /// 不显示图标
    None,
}

impl BackgroundIcon {
    /// 对应 config.toml 中的字符串值
    pub fn as_toml_str(self) -> &'static str {
        return match self {
            BackgroundIcon::Dock => "dock",
            BackgroundIcon::MenuBar => "menu-bar",
            BackgroundIcon::None => "none",
        };
    }
}

/// 托盘图标样式（Windows/Linux 生效）
///
/// macOS 不参与：其菜单栏图标为 template 图标，由系统按菜单栏明暗与高亮自动着色，
/// 配置值在 macOS 上被忽略（设置页也不展示该项）。
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrayIconStyle {
    /// 跟随系统：Windows 读任务栏深浅（SystemUsesLightTheme）自动选白/黑单色图标；
    /// Linux 无可靠的托盘底色检测手段，视作 Color（其默认值即 Color）
    Auto,
    /// 彩色应用图标（浅色/深色托盘底均可辨识）
    Color,
    /// 白色单色图标
    White,
    /// 黑色单色图标
    Black,
}

impl TrayIconStyle {
    /// 对应 config.toml 中的字符串值
    pub fn as_toml_str(self) -> &'static str {
        return match self {
            TrayIconStyle::Auto => "auto",
            TrayIconStyle::Color => "color",
            TrayIconStyle::White => "white",
            TrayIconStyle::Black => "black",
        };
    }
}

/// 主题模式：跟随系统 / 浅色 / 深色（暗色样式由前端根据此模式驱动）
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    /// 跟随系统
    System,
    /// 浅色
    Light,
    /// 深色
    Dark,
}impl ThemeMode {
    /// 对应 config.toml 中的字符串值
    pub fn as_toml_str(self) -> &'static str {
        return match self {
            ThemeMode::System => "system",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        };
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    theme: ThemeMode,
    #[serde(rename = "ankiConnectURL")]
    anki_connect_url: String,
    deck_name: String,
    model_name: String,
    auto_launch_anki: bool,
    anki_executable_path: String,
    global_shortcut: String,
    word_to_sentence: bool,
    keep_running_on_close: bool,
    background_icon: BackgroundIcon,
    tray_icon_style: TrayIconStyle,
    llm_enabled: bool,
    llm_base_url: String,
    llm_api_key: String,
    llm_model: String,
    llm_max_tokens: String,
    llm_reasoning_effort: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialConfig {
    theme: Option<ThemeMode>,
    #[serde(rename = "ankiConnectURL")]
    anki_connect_url: Option<String>,
    deck_name: Option<String>,
    model_name: Option<String>,
    auto_launch_anki: Option<bool>,
    anki_executable_path: Option<String>,
    global_shortcut: Option<String>,
    word_to_sentence: Option<bool>,
    keep_running_on_close: Option<bool>,
    background_icon: Option<BackgroundIcon>,
    tray_icon_style: Option<TrayIconStyle>,
    llm_enabled: Option<bool>,
    llm_base_url: Option<String>,
    llm_api_key: Option<String>,
    llm_model: Option<String>,
    llm_max_tokens: Option<String>,
    llm_reasoning_effort: Option<String>,
}

impl Config {
    /// 划词录入句子的全局快捷键，空字符串表示未设置
    pub fn global_shortcut(&self) -> &str {
        return &self.global_shortcut;
    }

    /// 选词取句：开启后划词只需选中一个单词，自动录入该词所在的整个句子（仅 macOS）
    pub fn word_to_sentence(&self) -> bool {
        return self.word_to_sentence;
    }

    /// 关闭窗口时应用是否保持后台运行（三端生效）
    pub fn keep_running_on_close(&self) -> bool {
        return self.keep_running_on_close;
    }

    /// 后台运行期间应用图标的显示位置（三端生效，dock 值仅 macOS 有 Dock 语义）
    pub fn background_icon(&self) -> BackgroundIcon {
        return self.background_icon;
    }

    /// 托盘图标样式（仅 Windows/Linux 生效；macOS 的菜单栏图标由系统 template 机制自动
    /// 适配明暗，该设置项在 macOS 上无意义，故访问器也只在非 macOS 编译）
    #[cfg(not(target_os = "macos"))]
    pub fn tray_icon_style(&self) -> TrayIconStyle {
        return self.tray_icon_style;
    }
}

impl Default for Config {
    /// 各键缺省值与 read_config 的缺省回退、配置模板保持一致，用于配置读取失败时的兜底。
    /// 文本键的存储缺省值为空串，“留空 = 使用内置默认”的生效值由前端消费端回退
    fn default() -> Self {
        return Config {
            theme: ThemeMode::System,
            anki_connect_url: String::new(),
            deck_name: String::new(),
            model_name: String::new(),
            auto_launch_anki: true,
            anki_executable_path: String::new(),
            global_shortcut: String::new(),
            word_to_sentence: true,
            keep_running_on_close: true,
            background_icon: BackgroundIcon::MenuBar,
            tray_icon_style: TrayIconStyle::Auto,
            llm_enabled: false,
            llm_base_url: String::new(),
            llm_api_key: String::new(),
            llm_model: String::new(),
            llm_max_tokens: String::new(),
            llm_reasoning_effort: String::new(),
        };
    }
}

/// 将配置模板复制到配置文件路径
pub fn copy_template_config(
    template_path: impl AsRef<Path>,
    config_path: impl AsRef<Path>,
) -> Result<(), String> {
    fn inner(config_path: &Path, template_path: &Path) -> Result<(), String> {
        let config_dir = config_path
            .parent()
            .ok_or("config path is a root or an empty string")?;
        std::fs::create_dir_all(config_dir)
            .map_err(|e| format!("failed to create directory {}: {e}", config_dir.display()))?;
        std::fs::copy(template_path, config_path).map_err(|e| {
            format!(
                "failed to copy template config from {} to {}: {e}",
                template_path.display(),
                config_path.display()
            )
        })?;
        return Ok(());
    }
    return inner(config_path.as_ref(), template_path.as_ref());
}

pub fn read_config(config_path: impl AsRef<Path>) -> Result<Config, String> {
    fn inner(config_path: &Path) -> Result<Config, String> {
        let toml_string = std::fs::read_to_string(config_path)
            .map_err(|e| format!("failed to read config file {}: {e}", config_path.display()))?;
        let doc = toml_string.parse::<toml_edit::DocumentMut>().map_err(|e| {
            format!(
                "failed to parse toml from config file {}: {e}",
                config_path.display()
            )
        })?;
        // AnkiConnect URL / 牌组名 / 模板名遵循“留空 = 使用内置默认”的语义（生效值由前端消费端回退），
        // 因此缺键（老模板或手工精简的配置文件）与非字符串值一律回退空串，绝不因缺键而报错
        let anki_connect_url = doc
            .get("anki-connect-url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // 主题模式为后加的键，老配置文件中没有；缺省或非法值回退跟随系统
        let theme = match doc.get("theme").and_then(|v| v.as_str()) {
            Some("light") => ThemeMode::Light,
            Some("dark") => ThemeMode::Dark,
            _ => ThemeMode::System,
        };
        let deck_name = doc
            .get("deck-name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let model_name = doc
            .get("model-name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // 新增键必须带缺省回退：老用户的配置文件中没有这些键，绝不能因缺键而报错
        let auto_launch_anki = doc
            .get("auto-launch-anki")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let anki_executable_path = doc
            .get("anki-executable-path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let global_shortcut = doc
            .get("global-shortcut")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // 选词取句为后加的键，老配置文件中没有；缺键或非法值回退开启
        let word_to_sentence = doc
            .get("word-to-sentence")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        // 关闭行为相关为后加的键，老配置文件中没有这些键，必须带缺省回退
        let keep_running_on_close = doc
            .get("keep-running-on-close")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        // 后台运行期间图标位置为后加的键，老配置文件中没有；缺省或非法值回退菜单栏图标
        let background_icon = match doc.get("background-icon").and_then(|v| v.as_str()) {
            Some("dock") => BackgroundIcon::Dock,
            Some("none") => BackgroundIcon::None,
            _ => BackgroundIcon::MenuBar,
        };
        // 托盘图标样式为后加的键，老配置文件中没有；缺省或非法值回退跟随系统
        // （该值带平台差异：auto 在 Linux 上等同 color，macOS 一律忽略）
        let tray_icon_style = match doc.get("tray-icon-style").and_then(|v| v.as_str()) {
            Some("color") => TrayIconStyle::Color,
            Some("white") => TrayIconStyle::White,
            Some("black") => TrayIconStyle::Black,
            _ => TrayIconStyle::Auto,
        };
        // AI 优选释义相关为后加的键，老配置文件中没有这些键，必须带缺省回退
        let llm_enabled = doc
            .get("llm-enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let llm_base_url = doc
            .get("llm-base-url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let llm_api_key = doc
            .get("llm-api-key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let llm_model = doc
            .get("llm-model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // 单次请求的最大生成 token 数（字符串存储，空串表示使用应用内置默认值）
        let llm_max_tokens = doc
            .get("llm-max-tokens")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let llm_reasoning_effort = doc
            .get("llm-reasoning-effort")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Ok(Config {
            theme,
            anki_connect_url,
            deck_name,
            model_name,
            auto_launch_anki,
            anki_executable_path,
            global_shortcut,
            word_to_sentence,
            keep_running_on_close,
            background_icon,
            tray_icon_style,
            llm_enabled,
            llm_base_url,
            llm_api_key,
            llm_model,
            llm_max_tokens,
            llm_reasoning_effort,
        });
    }
    return inner(config_path.as_ref());
}

pub fn commit_config(config_path: impl AsRef<Path>, modified: PartialConfig) -> Result<(), String> {
    fn inner(config_path: &Path, modified: PartialConfig) -> Result<(), String> {
        let toml_string = std::fs::read_to_string(config_path)
            .map_err(|e| format!("failed to read config file {}: {e}", config_path.display()))?;
        let mut doc = toml_string.parse::<toml_edit::DocumentMut>().map_err(|e| {
            format!(
                "failed to parse toml from config file {}: {e}",
                config_path.display()
            )
        })?;
        if let Some(theme) = modified.theme {
            doc["theme"] = toml_edit::value(theme.as_toml_str());
        }
        if let Some(anki_connect_url) = modified.anki_connect_url {
            doc["anki-connect-url"] = toml_edit::value(anki_connect_url);
        }
        if let Some(deck_name) = modified.deck_name {
            doc["deck-name"] = toml_edit::value(deck_name);
        }
        if let Some(model_name) = modified.model_name {
            doc["model-name"] = toml_edit::value(model_name);
        }
        if let Some(auto_launch_anki) = modified.auto_launch_anki {
            doc["auto-launch-anki"] = toml_edit::value(auto_launch_anki);
        }
        if let Some(anki_executable_path) = modified.anki_executable_path {
            doc["anki-executable-path"] = toml_edit::value(anki_executable_path);
        }
        if let Some(global_shortcut) = modified.global_shortcut {
            doc["global-shortcut"] = toml_edit::value(global_shortcut);
        }
        if let Some(word_to_sentence) = modified.word_to_sentence {
            doc["word-to-sentence"] = toml_edit::value(word_to_sentence);
        }
        if let Some(keep_running_on_close) = modified.keep_running_on_close {
            doc["keep-running-on-close"] = toml_edit::value(keep_running_on_close);
        }
        if let Some(background_icon) = modified.background_icon {
            doc["background-icon"] = toml_edit::value(background_icon.as_toml_str());
        }
        if let Some(tray_icon_style) = modified.tray_icon_style {
            doc["tray-icon-style"] = toml_edit::value(tray_icon_style.as_toml_str());
        }
        if let Some(llm_enabled) = modified.llm_enabled {
            doc["llm-enabled"] = toml_edit::value(llm_enabled);
        }
        if let Some(llm_base_url) = modified.llm_base_url {
            doc["llm-base-url"] = toml_edit::value(llm_base_url);
        }
        if let Some(llm_api_key) = modified.llm_api_key {
            doc["llm-api-key"] = toml_edit::value(llm_api_key);
        }
        if let Some(llm_model) = modified.llm_model {
            doc["llm-model"] = toml_edit::value(llm_model);
        }
        if let Some(llm_max_tokens) = modified.llm_max_tokens {
            doc["llm-max-tokens"] = toml_edit::value(llm_max_tokens);
        }
        if let Some(llm_reasoning_effort) = modified.llm_reasoning_effort {
            doc["llm-reasoning-effort"] = toml_edit::value(llm_reasoning_effort);
        }
        std::fs::write(config_path, doc.to_string()).map_err(|e| {
            format!(
                "failed to write to config file {}: {e}",
                config_path.display()
            )
        })?;
        return Ok(());
    }
    return inner(config_path.as_ref(), modified);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 配置模板必须能被 read_config 正确解析。
    ///
    /// 新增键的键名在三处出现（模板、read_config、commit_config），写错任一处都会**静默**
    /// 回退到缺省值而不报错；此处以真实模板为输入做回归，锁住模板与解析逻辑的一致性。
    #[test]
    fn reads_shipped_config_template() {
        let path = std::env::temp_dir().join("anki-marker-test-config-template.toml");
        std::fs::write(&path, include_str!("../../../resources/config-template.toml"))
            .expect("failed to write the temporary config file");
        let config = read_config(&path).expect("failed to parse the shipped config template");
        assert_eq!(config.tray_icon_style, TrayIconStyle::Auto);
        assert_eq!(config.background_icon, BackgroundIcon::MenuBar);
        assert_eq!(config.theme, ThemeMode::System);
        let _ = std::fs::remove_file(&path);
    }

    /// 前端以 camelCase 键名提交的托盘图标样式必须能落盘并被重新读出。
    /// （前端 JSON 键名 → PartialConfig 字段 → toml 键名 → read_config，任一环节写错都会
    /// 表现为“用户选了白色/黑色但托盘图标不变”）
    #[test]
    fn commits_and_reads_tray_icon_style() {
        let path = std::env::temp_dir().join("anki-marker-test-config-commit.toml");
        std::fs::write(&path, include_str!("../../../resources/config-template.toml"))
            .expect("failed to write the temporary config file");
        let modified: PartialConfig =
            serde_json::from_str(r#"{"trayIconStyle":"white"}"#).expect("failed to parse the payload");
        commit_config(&path, modified).expect("failed to commit the config");
        let config = read_config(&path).expect("failed to read back the config");
        assert_eq!(config.tray_icon_style, TrayIconStyle::White);
        let _ = std::fs::remove_file(&path);
    }
}

