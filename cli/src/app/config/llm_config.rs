use anyhow::Result;
use config::{AgentConfig, ReasoningEffort};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct UserLlmSettings {
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) reasoning_effort: ReasoningEffort,
}

impl UserLlmSettings {
    pub(crate) fn load(workspace_root: &Path) -> Result<Self> {
        let cfg = AgentConfig::load_runtime(workspace_root.to_path_buf())?;
        Ok(Self {
            api_key: cfg.llm.api_key,
            base_url: cfg.llm.base_url,
            model: cfg.llm.model,
            reasoning_effort: cfg.llm.model_reasoning_effort,
        })
    }

    pub(crate) fn save(&self, workspace_root: &Path) -> Result<()> {
        save_user_llm_settings(
            workspace_root,
            &self.api_key,
            &self.base_url,
            &self.model,
            self.reasoning_effort,
        )
    }
}

pub(crate) fn save_user_llm_settings(
    workspace_root: &Path,
    api_key: &str,
    base_url: &str,
    model: &str,
    reasoning_effort: ReasoningEffort,
) -> Result<()> {
    let path = active_config_path(workspace_root)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut document = if path.exists() {
        toml::from_str::<toml::Value>(&fs::read_to_string(&path)?)?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    let table = document
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("configuration root must be a TOML table"))?;
    let llm = table
        .entry("llm")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("configuration key `llm` must be a TOML table"))?;
    llm.insert(
        "api_key".to_string(),
        toml::Value::String(api_key.to_string()),
    );
    llm.insert(
        "base_url".to_string(),
        toml::Value::String(base_url.to_string()),
    );
    llm.insert("model".to_string(), toml::Value::String(model.to_string()));
    llm.insert(
        "model_reasoning_effort".to_string(),
        toml::Value::String(reasoning_effort.to_string()),
    );
    let body = toml::to_string_pretty(&document)?;
    fs::write(path, body)?;
    Ok(())
}

fn active_config_path(workspace_root: &Path) -> Result<PathBuf> {
    if !config::release_mode_enabled() {
        return Ok(workspace_root.join("configs").join("config.toml"));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("Cannot find user home directory"))?;
    Ok(PathBuf::from(home).join(".cloudagent").join("config.toml"))
}
