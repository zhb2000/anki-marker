use std::path::Path;

/// 后台运行（关闭窗口保持运行）期间应用图标的显示位置
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackgroundIcon {
    /// Dock 栏图标
    Dock,
    /// 屏幕顶部菜单栏图标
    MenuBar,
    /// 都不显示
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
}

impl ThemeMode {
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
    launch_anki_on_app_start: bool,
    anki_executable_path: String,
    global_shortcut: String,
    keep_running_on_close: bool,
    background_icon: BackgroundIcon,
    llm_enabled: bool,
    llm_base_url: String,
    llm_api_key: String,
    llm_model: String,
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
    launch_anki_on_app_start: Option<bool>,
    anki_executable_path: Option<String>,
    global_shortcut: Option<String>,
    keep_running_on_close: Option<bool>,
    background_icon: Option<BackgroundIcon>,
    llm_enabled: Option<bool>,
    llm_base_url: Option<String>,
    llm_api_key: Option<String>,
    llm_model: Option<String>,
    llm_reasoning_effort: Option<String>,
}

impl Config {
    /// 划词录入句子的全局快捷键，空字符串表示未设置
    pub fn global_shortcut(&self) -> &str {
        return &self.global_shortcut;
    }

    /// 关闭窗口时应用是否保持后台运行（仅 macOS 生效）
    pub fn keep_running_on_close(&self) -> bool {
        return self.keep_running_on_close;
    }

    /// 后台运行期间应用图标的显示位置（仅 macOS 生效）
    pub fn background_icon(&self) -> BackgroundIcon {
        return self.background_icon;
    }
}

impl Default for Config {
    /// 各键缺省值与 read_config 的缺省回退、配置模板保持一致，用于配置读取失败时的兜底
    fn default() -> Self {
        return Config {
            theme: ThemeMode::System,
            anki_connect_url: "http://localhost:8765".to_string(),
            deck_name: "划词助手默认牌组".to_string(),
            model_name: "划词助手默认单词模板".to_string(),
            auto_launch_anki: true,
            launch_anki_on_app_start: false,
            anki_executable_path: String::new(),
            global_shortcut: String::new(),
            keep_running_on_close: true,
            background_icon: BackgroundIcon::Dock,
            llm_enabled: false,
            llm_base_url: String::new(),
            llm_api_key: String::new(),
            llm_model: String::new(),
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
        let anki_connect_url = doc
            .get("anki-connect-url")
            .ok_or(r#"toml key "anki-connect-url" does not exist"#)?
            .as_str()
            .ok_or(r#"the value of "anki-connect-url" is not a string"#)?;
        // 主题模式为后加的键，老配置文件中没有；缺省或非法值回退跟随系统
        let theme = match doc.get("theme").and_then(|v| v.as_str()) {
            Some("light") => ThemeMode::Light,
            Some("dark") => ThemeMode::Dark,
            _ => ThemeMode::System,
        };
        let deck_name = doc
            .get("deck-name")
            .ok_or(r#"toml key "deck-name" does not exist"#)?
            .as_str()
            .ok_or(r#"the value of "deck-name" is not a string"#)?;
        let model_name = doc
            .get("model-name")
            .ok_or(r#"toml key "model-name" does not exist"#)?
            .as_str()
            .ok_or(r#"the value of "model-name" is not a string"#)?;
        // 新增键必须带缺省回退：老用户的配置文件中没有这些键，绝不能因缺键而报错
        let auto_launch_anki = doc
            .get("auto-launch-anki")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let launch_anki_on_app_start = doc
            .get("launch-anki-on-app-start")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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
        // 关闭行为相关为后加的键，老配置文件中没有这些键，必须带缺省回退
        let keep_running_on_close = doc
            .get("keep-running-on-close")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        // 后台运行期间图标位置为后加的键，老配置文件中没有；缺省或非法值回退 Dock
        let background_icon = match doc.get("background-icon").and_then(|v| v.as_str()) {
            Some("menu-bar") => BackgroundIcon::MenuBar,
            Some("none") => BackgroundIcon::None,
            _ => BackgroundIcon::Dock,
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
        let llm_reasoning_effort = doc
            .get("llm-reasoning-effort")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Ok(Config {
            theme,
            anki_connect_url: anki_connect_url.to_string(),
            deck_name: deck_name.to_string(),
            model_name: model_name.to_string(),
            auto_launch_anki,
            launch_anki_on_app_start,
            anki_executable_path,
            global_shortcut,
            keep_running_on_close,
            background_icon,
            llm_enabled,
            llm_base_url,
            llm_api_key,
            llm_model,
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
        if let Some(launch_anki_on_app_start) = modified.launch_anki_on_app_start {
            doc["launch-anki-on-app-start"] = toml_edit::value(launch_anki_on_app_start);
        }
        if let Some(anki_executable_path) = modified.anki_executable_path {
            doc["anki-executable-path"] = toml_edit::value(anki_executable_path);
        }
        if let Some(global_shortcut) = modified.global_shortcut {
            doc["global-shortcut"] = toml_edit::value(global_shortcut);
        }
        if let Some(keep_running_on_close) = modified.keep_running_on_close {
            doc["keep-running-on-close"] = toml_edit::value(keep_running_on_close);
        }
        if let Some(background_icon) = modified.background_icon {
            doc["background-icon"] = toml_edit::value(background_icon.as_toml_str());
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
