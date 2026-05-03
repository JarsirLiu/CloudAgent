use anyhow::{Context, Result, bail};
use agent_memory::{MemoryConfig, MemoryMode};
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
    pub memory: MemoryConfig,
    pub enable_skill_bucket: bool,
    pub enable_mcp_bucket: bool,
    pub post_compact_token_budget: usize,
    pub post_compact_memory_floor_tokens: usize,
    pub post_compact_skills_token_budget: usize,
    pub post_compact_mcp_token_budget: usize,
    pub post_compact_max_tokens_per_memory: usize,
    pub post_compact_max_tokens_per_skill: usize,
    pub post_compact_max_tokens_per_mcp: usize,
    pub context_budget_safety_buffer_tokens: usize,
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
    memory: Option<PartialMemoryConfig>,
    enable_skill_bucket: Option<bool>,
    enable_mcp_bucket: Option<bool>,
    post_compact_token_budget: Option<usize>,
    post_compact_memory_floor_tokens: Option<usize>,
    post_compact_skills_token_budget: Option<usize>,
    post_compact_mcp_token_budget: Option<usize>,
    post_compact_max_tokens_per_memory: Option<usize>,
    post_compact_max_tokens_per_skill: Option<usize>,
    post_compact_max_tokens_per_mcp: Option<usize>,
    context_budget_safety_buffer_tokens: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialMemoryConfig {
    enabled: Option<bool>,
    mode: Option<String>,
    root_dir: Option<PathBuf>,
    max_inject_chars: Option<usize>,
    min_turns_to_persist: Option<usize>,
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
                system_prompt: default_system_prompt(),
                max_tool_roundtrips: 12,
                conversation_store_dir: workspace_root.join("data").join("conversations"),
                model_context_window: 128_000,
                context_compaction_trigger_ratio: 0.90,
                context_compaction_target_tokens: 36_000,
                context_compaction_request_overhead_tokens: 28_000,
                context_compaction_preserved_user_turns: 3,
                context_compaction_preserved_tail_tokens: 12_000,
                context_compaction_summary_source_tokens: 24_000,
                memory: MemoryConfig::default(),
                enable_skill_bucket: false,
                enable_mcp_bucket: false,
                post_compact_token_budget: 50_000,
                post_compact_memory_floor_tokens: 6_000,
                post_compact_skills_token_budget: 25_000,
                post_compact_mcp_token_budget: 8_000,
                post_compact_max_tokens_per_memory: 6_000,
                post_compact_max_tokens_per_skill: 5_000,
                post_compact_max_tokens_per_mcp: 3_000,
                context_budget_safety_buffer_tokens: 8_000,
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
            if let Some(memory) = runtime.memory {
                if let Some(value) = memory.enabled {
                    self.runtime.memory.enabled = value;
                }
                if let Some(value) = memory.mode {
                    self.runtime.memory.mode = parse_memory_mode(&value);
                }
                if let Some(value) = memory.root_dir {
                    self.runtime.memory.root_dir = absolutize_path(&self.workspace_root, value);
                }
                if let Some(value) = memory.max_inject_chars {
                    self.runtime.memory.max_inject_chars = value.max(256);
                }
                if let Some(value) = memory.min_turns_to_persist {
                    self.runtime.memory.min_turns_to_persist = value.max(1);
                }
            }
            if let Some(value) = runtime.enable_skill_bucket {
                self.runtime.enable_skill_bucket = value;
            }
            if let Some(value) = runtime.enable_mcp_bucket {
                self.runtime.enable_mcp_bucket = value;
            }
            if let Some(value) = runtime.post_compact_token_budget {
                self.runtime.post_compact_token_budget = value.max(1_024);
            }
            if let Some(value) = runtime.post_compact_memory_floor_tokens {
                self.runtime.post_compact_memory_floor_tokens = value.max(512);
            }
            if let Some(value) = runtime.post_compact_skills_token_budget {
                self.runtime.post_compact_skills_token_budget = value.max(512);
            }
            if let Some(value) = runtime.post_compact_mcp_token_budget {
                self.runtime.post_compact_mcp_token_budget = value.max(512);
            }
            if let Some(value) = runtime.post_compact_max_tokens_per_memory {
                self.runtime.post_compact_max_tokens_per_memory = value.max(512);
            }
            if let Some(value) = runtime.post_compact_max_tokens_per_skill {
                self.runtime.post_compact_max_tokens_per_skill = value.max(512);
            }
            if let Some(value) = runtime.post_compact_max_tokens_per_mcp {
                self.runtime.post_compact_max_tokens_per_mcp = value.max(512);
            }
            if let Some(value) = runtime.context_budget_safety_buffer_tokens {
                self.runtime.context_budget_safety_buffer_tokens = value.max(512);
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
        if let Ok(value) = env::var("CLOUDAGENT_MEMORY_ENABLED")
            && let Ok(parsed) = value.parse::<bool>()
        {
            self.runtime.memory.enabled = parsed;
        }
        if let Ok(value) = env::var("CLOUDAGENT_MEMORY_MODE") {
            self.runtime.memory.mode = parse_memory_mode(&value);
        }
        if let Ok(value) = env::var("CLOUDAGENT_ENABLE_SKILL_BUCKET")
            && let Ok(parsed) = value.parse::<bool>()
        {
            self.runtime.enable_skill_bucket = parsed;
        }
        if let Ok(value) = env::var("CLOUDAGENT_ENABLE_MCP_BUCKET")
            && let Ok(parsed) = value.parse::<bool>()
        {
            self.runtime.enable_mcp_bucket = parsed;
        }
        if let Ok(value) = env::var("CLOUDAGENT_POST_COMPACT_TOKEN_BUDGET")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.post_compact_token_budget = parsed.max(1_024);
        }
        if let Ok(value) = env::var("CLOUDAGENT_POST_COMPACT_MEMORY_FLOOR_TOKENS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.post_compact_memory_floor_tokens = parsed.max(512);
        }
        if let Ok(value) = env::var("CLOUDAGENT_POST_COMPACT_SKILLS_TOKEN_BUDGET")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.post_compact_skills_token_budget = parsed.max(512);
        }
        if let Ok(value) = env::var("CLOUDAGENT_POST_COMPACT_MCP_TOKEN_BUDGET")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.post_compact_mcp_token_budget = parsed.max(512);
        }
        if let Ok(value) = env::var("CLOUDAGENT_CONTEXT_BUDGET_SAFETY_BUFFER_TOKENS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.context_budget_safety_buffer_tokens = parsed.max(512);
        }
    }
}

fn parse_memory_mode(value: &str) -> MemoryMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "basic" => MemoryMode::Basic,
        "evolve" => MemoryMode::Evolve,
        _ => MemoryMode::Off,
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
        "You are cloudagent, a coding and operations agent focused on delivering correct, complete outcomes with minimal back-and-forth.",
        "Start by understanding the task and relevant code path before editing.",
        "Prefer focused, high-signal actions over broad exploration, and keep changes minimal, consistent, and scoped to the request.",
        "For repository work, use this order by default: locate candidates, read targeted files, then run precise commands only when needed.",
        "If a search returns weak results, broaden scope before repeating the same query.",
        "Use platform-appropriate commands and workspace-relative paths unless absolute paths are explicitly required.",
        "Prefer safe, read-first workflows before mutating actions.",
        "Before modifying code or creating commits, align with the repository's existing style, conventions, and project workflow; do not make arbitrary edits or commits.",
        "Do not introduce compatibility shims, fallback patches, or risk-bearing workaround designs without explicit user alignment on a clear plan, unless the user has clearly delegated solution choice.",
        "After making changes, run the most relevant narrow validation available (tests, build, or lint) when feasible, then expand only if needed.",
    ]
    .join(" ")
}
