use std::path::Path;

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(rename = "ankiConnectURL")]
    anki_connect_url: String,
    deck_name: String,
    model_name: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialConfig {
    #[serde(rename = "ankiConnectURL")]
    anki_connect_url: Option<String>,
    deck_name: Option<String>,
    model_name: Option<String>,
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
        return Ok(Config {
            anki_connect_url: anki_connect_url.to_string(),
            deck_name: deck_name.to_string(),
            model_name: model_name.to_string(),
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
        if let Some(anki_connect_url) = modified.anki_connect_url {
            doc["anki-connect-url"] = toml_edit::value(anki_connect_url);
        }
        if let Some(deck_name) = modified.deck_name {
            doc["deck-name"] = toml_edit::value(deck_name);
        }
        if let Some(model_name) = modified.model_name {
            doc["model-name"] = toml_edit::value(model_name);
        }
        std::fs::write(&config_path, doc.to_string()).map_err(|e| {
            format!(
                "failed to write to config file {}: {e}",
                config_path.display()
            )
        })?;
        return Ok(());
    }
    return inner(config_path.as_ref(), modified);
}
