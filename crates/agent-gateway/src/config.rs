use anyhow::{Context, Result, bail};
use config::AgentConfig;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GatewayConfigFile {
    #[serde(default)]
    pub gateway: GatewayServerConfigFile,
    #[serde(default)]
    pub feishu: FeishuConfigFile,
    #[serde(default)]
    pub llm: LlmConfigFile,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayServerConfigFile {
    pub log_filter: Option<String>,
}

impl Default for GatewayServerConfigFile {
    fn default() -> Self {
        Self {
            log_filter: Some("info".to_string()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FeishuConfigFile {
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
    pub verification_token: Option<String>,
    pub encrypt_key: Option<String>,
    pub base_url: Option<String>,
    pub group_only_mentioned: Option<bool>,
    pub group_reply_without_mention: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct FeishuPlatformFile {
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
    pub verification_token: Option<String>,
    pub encrypt_key: Option<String>,
    pub base_url: Option<String>,
    pub group_reply_without_mention: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LlmConfigFile {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub log_filter: String,
    pub feishu: FeishuConfig,
    pub llm: LlmConfig,
}

#[derive(Debug, Clone)]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    pub verification_token: Option<String>,
    pub encrypt_key: Option<String>,
    pub base_url: String,
    pub group_only_mentioned: bool,
    pub group_reply_without_mention: bool,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f32,
    pub system_prompt: String,
}

pub fn load_gateway_config(explicit_path: Option<&Path>) -> Result<GatewayConfig> {
    let file = load_config_file(explicit_path)?;
    let platform_feishu = load_platform_feishu_file()?;

    let log_filter = env_or(
        "GATEWAY_LOG_FILTER",
        file.gateway.log_filter.clone(),
        "info".to_string(),
    );

    let feishu = FeishuConfig {
        app_id: required(
            "FEISHU_APP_ID",
            env_or_opt(
                "FEISHU_APP_ID",
                first_some(file.feishu.app_id.clone(), platform_feishu.app_id.clone()),
            ),
        )?,
        app_secret: required(
            "FEISHU_APP_SECRET",
            env_or_opt(
                "FEISHU_APP_SECRET",
                first_some(
                    file.feishu.app_secret.clone(),
                    platform_feishu.app_secret.clone(),
                ),
            ),
        )?,
        verification_token: env_or_opt(
            "FEISHU_VERIFICATION_TOKEN",
            first_some(
                file.feishu.verification_token.clone(),
                platform_feishu.verification_token.clone(),
            ),
        ),
        encrypt_key: env_or_opt(
            "FEISHU_ENCRYPT_KEY",
            first_some(
                file.feishu.encrypt_key.clone(),
                platform_feishu.encrypt_key.clone(),
            ),
        ),
        base_url: env_or(
            "FEISHU_BASE_URL",
            first_some(
                file.feishu.base_url.clone(),
                platform_feishu.base_url.clone(),
            ),
            "https://open.feishu.cn".to_string(),
        ),
        group_only_mentioned: env_bool(
            "FEISHU_GROUP_ONLY_MENTIONED",
            file.feishu.group_only_mentioned,
            true,
        ),
        group_reply_without_mention: env_bool(
            "FEISHU_GROUP_REPLY_WITHOUT_MENTION",
            file.feishu
                .group_reply_without_mention
                .or(platform_feishu.group_reply_without_mention),
            true,
        ),
    };

    let llm = LlmConfig {
        base_url: normalize_base_url(&env_or(
            "OPENAI_BASE_URL",
            env_or_opt("LLM_BASE_URL", file.llm.base_url.clone()),
            "https://api.openai.com/v1".to_string(),
        )),
        api_key: required(
            "OPENAI_API_KEY",
            env_or_opt("OPENAI_API_KEY", env_or_opt("LLM_API_KEY", file.llm.api_key.clone())),
        )?,
        model: env_or(
            "OPENAI_MODEL",
            env_or_opt("LLM_MODEL", file.llm.model.clone()),
            "gpt-4.1-mini".to_string(),
        ),
        temperature: env_float(
            "LLM_TEMPERATURE",
            file.llm.temperature,
            0.2,
        ),
        system_prompt: env_or(
            "LLM_SYSTEM_PROMPT",
            file.llm.system_prompt.clone(),
            "You are CloudAgent running inside Feishu. Give concise, practical answers in Chinese unless the user asks otherwise.".to_string(),
        ),
    };

    Ok(GatewayConfig {
        log_filter,
        feishu,
        llm,
    })
}

fn load_config_file(explicit_path: Option<&Path>) -> Result<GatewayConfigFile> {
    let mut merged = toml::Value::Table(toml::map::Map::new());
    let mut found = false;
    for path in config_paths(explicit_path) {
        if !path.exists() {
            continue;
        }
        found = true;
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let value = toml::from_str::<toml::Value>(&raw)
            .with_context(|| format!("failed to parse config file {}", path.display()))?;
        merge_toml_values(&mut merged, value);
    }
    if !found {
        return Ok(GatewayConfigFile::default());
    }
    toml::from_str::<GatewayConfigFile>(&toml::to_string(&merged)?)
        .context("failed to decode merged gateway configuration")
}

fn load_platform_feishu_file() -> Result<FeishuPlatformFile> {
    let path = resolve_platform_config_file("feishu.json")?;
    if !path.exists() {
        return Ok(FeishuPlatformFile::default());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read platform config {}", path.display()))?;
    let parsed = serde_json::from_str::<FeishuPlatformFile>(&raw)
        .with_context(|| format!("failed to parse platform config {}", path.display()))?;
    Ok(parsed)
}

fn resolve_platform_config_file(file_name: &str) -> Result<PathBuf> {
    let workspace_root = env::current_dir().context("failed to determine current directory")?;
    let agent_config = AgentConfig::load_runtime(workspace_root)?;
    let data_root = agent_config.runtime.data_root_dir;
    let platform_dir = match (
        data_root.file_name().and_then(|name| name.to_str()),
        data_root
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str()),
    ) {
        (Some("data"), Some(".cloudagent")) => data_root
            .parent()
            .map(|parent| parent.join("platform"))
            .unwrap_or_else(|| data_root.join("platform")),
        _ => data_root.join("platform"),
    };
    Ok(platform_dir.join(file_name))
}

fn config_paths(explicit_path: Option<&Path>) -> Vec<PathBuf> {
    if let Some(path) = explicit_path {
        return vec![path.to_path_buf()];
    }

    if let Ok(value) = env::var("CLOUDAGENT_CONFIG")
        && !value.trim().is_empty()
    {
        return vec![PathBuf::from(value)];
    }

    let Some(cwd) = env::current_dir().ok() else {
        return Vec::new();
    };
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from);

    if config::release_mode_enabled() {
        return home
            .map(|path| vec![path.join(".cloudagent").join("config.toml")])
            .unwrap_or_default();
    }

    // Keep the same low-to-high precedence as AgentConfig::load: user config
    // is the base, while the checked-out project's config is authoritative.
    [
        home.map(|path| path.join(".cloudagent").join("config.toml")),
        Some(cwd.join(".cloudagent").join("config.toml")),
        Some(cwd.join("configs").join("config.toml")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn merge_toml_values(target: &mut toml::Value, overlay: toml::Value) {
    match (target, overlay) {
        (toml::Value::Table(target), toml::Value::Table(overlay)) => {
            for (key, value) in overlay {
                if let Some(existing) = target.get_mut(&key) {
                    merge_toml_values(existing, value);
                } else {
                    target.insert(key, value);
                }
            }
        }
        (target, overlay) => *target = overlay,
    }
}

fn required(name: &str, value: Option<String>) -> Result<String> {
    match value.map(|item| item.trim().to_string()) {
        Some(value) if !value.is_empty() => Ok(value),
        _ => bail!(
            "{name} is required. Checked env vars, config.toml, and the resolved platform config file. \
Set it in configs/config.toml under [feishu], configure it via /gateway, or export {name}."
        ),
    }
}

fn env_or(name: &str, file_value: Option<String>, default: String) -> String {
    env_or_opt(name, file_value).unwrap_or(default)
}

fn env_or_opt(name: &str, file_value: Option<String>) -> Option<String> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => file_value,
    }
}

fn first_some(primary: Option<String>, fallback: Option<String>) -> Option<String> {
    match primary {
        Some(value) if !value.trim().is_empty() => Some(value),
        _ => fallback,
    }
}

fn env_bool(name: &str, file_value: Option<bool>, default: bool) -> bool {
    match env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => file_value.unwrap_or(default),
    }
}

fn env_float(name: &str, file_value: Option<f32>, default: f32) -> f32 {
    match env::var(name) {
        Ok(value) => value.parse::<f32>().unwrap_or(default),
        Err(_) => file_value.unwrap_or(default),
    }
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}
