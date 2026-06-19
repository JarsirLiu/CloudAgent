use anyhow::Result;
use config::{AgentConfig, ReasoningEffort};
use serde::Serialize;
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
        let cfg = AgentConfig::load_user_only(workspace_root.to_path_buf())?;
        Ok(Self {
            api_key: cfg.llm.api_key,
            base_url: cfg.llm.base_url,
            model: cfg.llm.model,
            reasoning_effort: cfg.llm.model_reasoning_effort,
        })
    }

    pub(crate) fn save(&self) -> Result<()> {
        save_user_llm_settings(
            &self.api_key,
            &self.base_url,
            &self.model,
            self.reasoning_effort,
        )
    }
}

pub(crate) fn save_user_llm_settings(
    api_key: &str,
    base_url: &str,
    model: &str,
    reasoning_effort: ReasoningEffort,
) -> Result<()> {
    let path = user_llm_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = toml::to_string(&UserConfigFile {
        llm: UserLlmConfig {
            api_key,
            base_url,
            model,
            model_reasoning_effort: reasoning_effort,
        },
    })?;
    fs::write(path, body)?;
    Ok(())
}

fn user_llm_config_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("Cannot find user home directory"))?;
    Ok(PathBuf::from(home).join(".cloudagent").join("config.toml"))
}

#[derive(Serialize)]
struct UserConfigFile<'a> {
    llm: UserLlmConfig<'a>,
}

#[derive(Serialize)]
struct UserLlmConfig<'a> {
    api_key: &'a str,
    base_url: &'a str,
    model: &'a str,
    model_reasoning_effort: ReasoningEffort,
}
