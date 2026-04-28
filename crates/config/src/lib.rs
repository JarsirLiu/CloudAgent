use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    pub workspace_root: PathBuf,
    pub llm: LlmConfig,
    pub runtime: RuntimeConfig,
    pub tools: ToolConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub default_session_id: String,
    pub system_prompt: String,
    pub max_tool_roundtrips: usize,
    pub session_store_dir: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolConfig {
    pub default_shell_timeout_ms: u64,
    pub max_read_chars: usize,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialAgentConfig {
    llm: Option<PartialLlmConfig>,
    runtime: Option<PartialRuntimeConfig>,
    tools: Option<PartialToolConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialLlmConfig {
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    temperature: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialRuntimeConfig {
    default_session_id: Option<String>,
    system_prompt: Option<String>,
    max_tool_roundtrips: Option<usize>,
    session_store_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialToolConfig {
    default_shell_timeout_ms: Option<u64>,
    max_read_chars: Option<usize>,
}

impl AgentConfig {
    pub fn load(workspace_root: impl Into<PathBuf>) -> Result<Self> {
        let workspace_root = workspace_root.into();
        let mut config = Self::defaults(workspace_root.clone());
        let config_path = workspace_root.join("configs").join("agent.toml");

        if config_path.exists() {
            let text = std::fs::read_to_string(&config_path)
                .with_context(|| format!("failed to read {}", config_path.display()))?;
            let partial: PartialAgentConfig = toml::from_str(&text)
                .with_context(|| format!("failed to parse {}", config_path.display()))?;
            config.apply_partial(partial);
        }

        config.apply_env_overrides();
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.llm.base_url.trim().is_empty() {
            bail!("llm.base_url cannot be empty");
        }
        if self.llm.model.trim().is_empty() {
            bail!("llm.model cannot be empty");
        }
        if self.llm.api_key.trim().is_empty() {
            bail!(
                "missing LLM api key; set CLOUDAGENT_LLM_API_KEY or configs/agent.toml -> llm.api_key"
            );
        }
        Ok(())
    }

    fn defaults(workspace_root: PathBuf) -> Self {
        Self {
            runtime: RuntimeConfig {
                default_session_id: "default".to_string(),
                system_prompt: default_system_prompt(),
                max_tool_roundtrips: 8,
                session_store_dir: workspace_root.join("data").join("sessions"),
            },
            llm: LlmConfig {
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: String::new(),
                model: "gpt-4.1-mini".to_string(),
                temperature: 0.2,
            },
            tools: ToolConfig {
                default_shell_timeout_ms: 120_000,
                max_read_chars: 20_000,
            },
            workspace_root,
        }
    }

    fn apply_partial(&mut self, partial: PartialAgentConfig) {
        if let Some(llm) = partial.llm {
            if let Some(value) = llm.base_url {
                self.llm.base_url = value;
            }
            if let Some(value) = llm.api_key {
                self.llm.api_key = value;
            }
            if let Some(value) = llm.model {
                self.llm.model = value;
            }
            if let Some(value) = llm.temperature {
                self.llm.temperature = value;
            }
        }

        if let Some(runtime) = partial.runtime {
            if let Some(value) = runtime.default_session_id {
                self.runtime.default_session_id = value;
            }
            if let Some(value) = runtime.system_prompt {
                self.runtime.system_prompt = value;
            }
            if let Some(value) = runtime.max_tool_roundtrips {
                self.runtime.max_tool_roundtrips = value.max(1);
            }
            if let Some(value) = runtime.session_store_dir {
                self.runtime.session_store_dir = absolutize_path(&self.workspace_root, value);
            }
        }

        if let Some(tools) = partial.tools {
            if let Some(value) = tools.default_shell_timeout_ms {
                self.tools.default_shell_timeout_ms = value.max(1_000);
            }
            if let Some(value) = tools.max_read_chars {
                self.tools.max_read_chars = value.max(1_024);
            }
        }
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(value) = env::var("CLOUDAGENT_LLM_BASE_URL") {
            self.llm.base_url = value;
        }
        if let Ok(value) = env::var("CLOUDAGENT_LLM_API_KEY") {
            self.llm.api_key = value;
        }
        if let Ok(value) = env::var("CLOUDAGENT_LLM_MODEL") {
            self.llm.model = value;
        }
        if let Ok(value) = env::var("CLOUDAGENT_LLM_TEMPERATURE")
            && let Ok(parsed) = value.parse::<f32>()
        {
            self.llm.temperature = parsed;
        }
        if let Ok(value) = env::var("CLOUDAGENT_SYSTEM_PROMPT") {
            self.runtime.system_prompt = value;
        }
        if let Ok(value) = env::var("CLOUDAGENT_DEFAULT_SESSION_ID") {
            self.runtime.default_session_id = value;
        }
        if let Ok(value) = env::var("CLOUDAGENT_MAX_TOOL_ROUNDTRIPS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.max_tool_roundtrips = parsed.max(1);
        }
        if let Ok(value) = env::var("CLOUDAGENT_SESSION_STORE_DIR") {
            self.runtime.session_store_dir =
                absolutize_path(&self.workspace_root, PathBuf::from(value));
        }
        if let Ok(value) = env::var("CLOUDAGENT_SHELL_TIMEOUT_MS")
            && let Ok(parsed) = value.parse::<u64>()
        {
            self.tools.default_shell_timeout_ms = parsed.max(1_000);
        }
        if let Ok(value) = env::var("CLOUDAGENT_MAX_READ_CHARS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.tools.max_read_chars = parsed.max(1_024);
        }
    }
}

fn absolutize_path(workspace_root: &Path, value: PathBuf) -> PathBuf {
    if value.is_absolute() {
        value
    } else {
        workspace_root.join(value)
    }
}

fn default_system_prompt() -> String {
    [
        "You are cloudagent, a server-oriented autonomous coding and operations agent.",
        "Work in iterative turns, keep track of the ongoing conversation, and use tools when real inspection or file changes are needed.",
        "Prefer inspecting the environment before making claims, explain your reasoning briefly, and keep outputs actionable.",
        "When editing files or writing scripts, be explicit about the paths you changed or created.",
        "If a tool result is ambiguous or incomplete, ask a focused follow-up question or run another tool instead of guessing.",
    ]
    .join(" ")
}
