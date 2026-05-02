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
    pub default_conversation_id: String,
    pub system_prompt: String,
    pub max_tool_roundtrips: usize,
    pub conversation_store_dir: PathBuf,
    pub model_context_window: u64,
    pub context_compaction_trigger_ratio: f32,
    pub context_compaction_target_tokens: usize,
    pub context_compaction_request_overhead_tokens: usize,
    pub context_compaction_preserved_user_turns: usize,
    pub context_compaction_preserved_tail_tokens: usize,
    pub context_compaction_summary_source_tokens: usize,
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
    #[serde(alias = "default_session_id")]
    default_conversation_id: Option<String>,
    system_prompt: Option<String>,
    max_tool_roundtrips: Option<usize>,
    #[serde(alias = "session_store_dir")]
    conversation_store_dir: Option<PathBuf>,
    model_context_window: Option<u64>,
    context_compaction_trigger_ratio: Option<f32>,
    context_compaction_target_tokens: Option<usize>,
    context_compaction_request_overhead_tokens: Option<usize>,
    context_compaction_preserved_user_turns: Option<usize>,
    context_compaction_preserved_tail_tokens: Option<usize>,
    context_compaction_summary_source_tokens: Option<usize>,
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
                default_conversation_id: "default".to_string(),
                system_prompt: default_system_prompt(),
                max_tool_roundtrips: 12,
                conversation_store_dir: workspace_root.join("data").join("conversations"),
                model_context_window: 128_000,
                context_compaction_trigger_ratio: 0.85,
                context_compaction_target_tokens: 36_000,
                context_compaction_request_overhead_tokens: 28_000,
                context_compaction_preserved_user_turns: 3,
                context_compaction_preserved_tail_tokens: 12_000,
                context_compaction_summary_source_tokens: 24_000,
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
            if let Some(value) = runtime.default_conversation_id {
                self.runtime.default_conversation_id = value;
            }
            if let Some(value) = runtime.system_prompt {
                self.runtime.system_prompt = value;
            }
            if let Some(value) = runtime.max_tool_roundtrips {
                self.runtime.max_tool_roundtrips = value.max(1);
            }
            if let Some(value) = runtime.conversation_store_dir {
                self.runtime.conversation_store_dir = absolutize_path(&self.workspace_root, value);
            }
            if let Some(value) = runtime.model_context_window {
                self.runtime.model_context_window = value.max(2_048);
            }
            if let Some(value) = runtime.context_compaction_trigger_ratio {
                self.runtime.context_compaction_trigger_ratio = value.clamp(0.5, 0.98);
            }
            if let Some(value) = runtime.context_compaction_target_tokens {
                self.runtime.context_compaction_target_tokens = value.max(512);
            }
            if let Some(value) = runtime.context_compaction_request_overhead_tokens {
                self.runtime.context_compaction_request_overhead_tokens = value;
            }
            if let Some(value) = runtime.context_compaction_preserved_user_turns {
                self.runtime.context_compaction_preserved_user_turns = value.clamp(1, 12);
            }
            if let Some(value) = runtime.context_compaction_preserved_tail_tokens {
                self.runtime.context_compaction_preserved_tail_tokens = value.max(512);
            }
            if let Some(value) = runtime.context_compaction_summary_source_tokens {
                self.runtime.context_compaction_summary_source_tokens = value.max(1_024);
            }
            let trigger_tokens = ((self.runtime.model_context_window as f32)
                * self.runtime.context_compaction_trigger_ratio)
                as usize;
            if self.runtime.context_compaction_request_overhead_tokens >= trigger_tokens {
                self.runtime.context_compaction_request_overhead_tokens =
                    trigger_tokens.saturating_sub(4_000);
            }
            if self.runtime.context_compaction_target_tokens >= trigger_tokens {
                self.runtime.context_compaction_target_tokens =
                    trigger_tokens.saturating_sub(8_000).max(512);
            }
            if self
                .runtime
                .context_compaction_target_tokens
                .saturating_add(self.runtime.context_compaction_request_overhead_tokens)
                >= trigger_tokens
            {
                self.runtime.context_compaction_target_tokens = trigger_tokens
                    .saturating_sub(self.runtime.context_compaction_request_overhead_tokens)
                    .saturating_sub(4_000)
                    .max(512);
            }
            if self.runtime.context_compaction_preserved_tail_tokens
                >= self.runtime.context_compaction_target_tokens
            {
                self.runtime.context_compaction_preserved_tail_tokens = self
                    .runtime
                    .context_compaction_target_tokens
                    .saturating_sub(8_000)
                    .max(512);
            }
            if self.runtime.context_compaction_summary_source_tokens
                >= self.runtime.model_context_window as usize
            {
                self.runtime.context_compaction_summary_source_tokens =
                    (self.runtime.model_context_window as usize)
                        .saturating_sub(8_000)
                        .max(1_024);
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
        if let Ok(value) = env::var("CLOUDAGENT_DEFAULT_CONVERSATION_ID") {
            self.runtime.default_conversation_id = value;
        }
        if let Ok(value) = env::var("CLOUDAGENT_MAX_TOOL_ROUNDTRIPS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.max_tool_roundtrips = parsed.max(1);
        }
        if let Ok(value) = env::var("CLOUDAGENT_CONVERSATION_STORE_DIR") {
            self.runtime.conversation_store_dir =
                absolutize_path(&self.workspace_root, PathBuf::from(value));
        }
        if let Ok(value) = env::var("CLOUDAGENT_MODEL_CONTEXT_WINDOW")
            && let Ok(parsed) = value.parse::<u64>()
        {
            self.runtime.model_context_window = parsed.max(2_048);
        }
        if let Ok(value) = env::var("CLOUDAGENT_CONTEXT_COMPACTION_TRIGGER_RATIO")
            && let Ok(parsed) = value.parse::<f32>()
        {
            self.runtime.context_compaction_trigger_ratio = parsed.clamp(0.5, 0.98);
        }
        if let Ok(value) = env::var("CLOUDAGENT_CONTEXT_COMPACTION_TARGET_TOKENS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.context_compaction_target_tokens = parsed.max(512);
        }
        if let Ok(value) = env::var("CLOUDAGENT_CONTEXT_COMPACTION_REQUEST_OVERHEAD_TOKENS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.context_compaction_request_overhead_tokens = parsed;
        }
        if let Ok(value) = env::var("CLOUDAGENT_CONTEXT_COMPACTION_PRESERVED_USER_TURNS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.context_compaction_preserved_user_turns = parsed.clamp(1, 12);
        }
        if let Ok(value) = env::var("CLOUDAGENT_CONTEXT_COMPACTION_PRESERVED_TAIL_TOKENS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.context_compaction_preserved_tail_tokens = parsed.max(512);
        }
        if let Ok(value) = env::var("CLOUDAGENT_CONTEXT_COMPACTION_SUMMARY_SOURCE_TOKENS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.context_compaction_summary_source_tokens = parsed.max(1_024);
        }
        let trigger_tokens = ((self.runtime.model_context_window as f32)
            * self.runtime.context_compaction_trigger_ratio) as usize;
        if self.runtime.context_compaction_request_overhead_tokens >= trigger_tokens {
            self.runtime.context_compaction_request_overhead_tokens =
                trigger_tokens.saturating_sub(4_000);
        }
        if self.runtime.context_compaction_target_tokens >= trigger_tokens {
            self.runtime.context_compaction_target_tokens =
                trigger_tokens.saturating_sub(8_000).max(512);
        }
        if self
            .runtime
            .context_compaction_target_tokens
            .saturating_add(self.runtime.context_compaction_request_overhead_tokens)
            >= trigger_tokens
        {
            self.runtime.context_compaction_target_tokens = trigger_tokens
                .saturating_sub(self.runtime.context_compaction_request_overhead_tokens)
                .saturating_sub(4_000)
                .max(512);
        }
        if self.runtime.context_compaction_preserved_tail_tokens
            >= self.runtime.context_compaction_target_tokens
        {
            self.runtime.context_compaction_preserved_tail_tokens = self
                .runtime
                .context_compaction_target_tokens
                .saturating_sub(8_000)
                .max(512);
        }
        if self.runtime.context_compaction_summary_source_tokens
            >= self.runtime.model_context_window as usize
        {
            self.runtime.context_compaction_summary_source_tokens =
                (self.runtime.model_context_window as usize)
                    .saturating_sub(8_000)
                    .max(1_024);
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
        "When exploring a repository, prefer high-information inspection over repeated directory browsing.",
        "Batch independent tool calls in the same round when possible instead of returning to the model after each small step.",
        "After locating a relevant directory, prefer reading likely files or searching for relevant code over continuing to list subdirectories.",
        "Do not spend multiple consecutive rounds only enumerating directories if enough context exists to inspect files.",
        "When asked how a mechanism works, provide an initial structural answer as soon as the evidence is sufficient, then deepen it if needed.",
    ]
    .join(" ")
}
