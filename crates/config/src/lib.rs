use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use shared::{MemoryConfig, MemoryMode};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputModality {
    Text,
    Image,
}

pub fn default_input_modalities() -> Vec<InputModality> {
    vec![InputModality::Text, InputModality::Image]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    pub workspace_root: PathBuf,
    pub llm: LlmConfig,
    pub runtime: RuntimeConfig,
    pub tools: ToolConfig,
    pub cli: CliConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_input_modalities")]
    pub input_modalities: Vec<InputModality>,
    pub temperature: f32,
    pub request_max_retries: u64,
    pub stream_max_retries: u64,
    pub stream_idle_timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub system_prompt: String,
    pub max_tool_roundtrips: Option<usize>,
    pub data_root_dir: PathBuf,
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
    pub mcp_servers: Vec<McpServerConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub startup_timeout_ms: u64,
    pub supports_parallel_tool_calls: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CliConfig {
    pub pre_llm_filter_enabled: bool,
    pub permission_mode: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialAgentConfig {
    llm: Option<PartialLlmConfig>,
    runtime: Option<PartialRuntimeConfig>,
    tools: Option<PartialToolConfig>,
    cli: Option<PartialCliConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialLlmConfig {
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    input_modalities: Option<Vec<InputModality>>,
    temperature: Option<f32>,
    request_max_retries: Option<u64>,
    stream_max_retries: Option<u64>,
    stream_idle_timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialRuntimeConfig {
    system_prompt: Option<String>,
    max_tool_roundtrips: Option<Option<usize>>,
    data_root_dir: Option<PathBuf>,
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
    mcp_servers: Option<Vec<PartialMcpServerConfig>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialMcpServerConfig {
    name: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<BTreeMap<String, String>>,
    cwd: Option<Option<PathBuf>>,
    startup_timeout_ms: Option<u64>,
    supports_parallel_tool_calls: Option<bool>,
    enabled: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialCliConfig {
    pre_llm_filter_enabled: Option<bool>,
    permission_mode: Option<String>,
}

impl AgentConfig {
    pub fn load(workspace_root: impl Into<PathBuf>) -> Result<Self> {
        let workspace_root = workspace_root.into();
        let mut config = Self::defaults(workspace_root.clone());
        for config_path in config_search_paths(&workspace_root) {
            if config_path.exists() {
                let text = std::fs::read_to_string(&config_path)
                    .with_context(|| format!("failed to read {}", config_path.display()))?;
                let partial: PartialAgentConfig = toml::from_str(&text)
                    .with_context(|| format!("failed to parse {}", config_path.display()))?;
                config.apply_partial(partial);
            }
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
            bail!("missing LLM api key; set CLOUDAGENT_LLM_API_KEY or config.toml -> llm.api_key");
        }
        let mut seen_mcp_servers = std::collections::BTreeSet::new();
        for server in &self.tools.mcp_servers {
            if server.name.trim().is_empty() {
                bail!("tools.mcp_servers[*].name cannot be empty");
            }
            if server.command.trim().is_empty() {
                bail!("tools.mcp_servers[{}].command cannot be empty", server.name);
            }
            if !seen_mcp_servers.insert(server.name.clone()) {
                bail!("duplicate MCP server name `{}`", server.name);
            }
        }
        Ok(())
    }

    fn defaults(workspace_root: PathBuf) -> Self {
        let data_root_dir = workspace_root.join("data");
        let memory = MemoryConfig {
            root_dir: data_root_dir.join("state").join("memory"),
            ..MemoryConfig::default()
        };
        Self {
            runtime: RuntimeConfig {
                system_prompt: default_system_prompt(),
                max_tool_roundtrips: None,
                data_root_dir: data_root_dir.clone(),
                conversation_store_dir: data_root_dir.join("conversations"),
                model_context_window: 200_000,
                context_compaction_trigger_ratio: 0.90,
                context_compaction_target_tokens: 36_000,
                context_compaction_request_overhead_tokens: 28_000,
                context_compaction_preserved_user_turns: 3,
                context_compaction_preserved_tail_tokens: 12_000,
                context_compaction_summary_source_tokens: 24_000,
                memory,
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
                model: "gpt-5.4".to_string(),
                input_modalities: default_input_modalities(),
                temperature: 0.2,
                request_max_retries: 0,
                stream_max_retries: 0,
                stream_idle_timeout_ms: 30_000,
            },
            tools: ToolConfig {
                default_shell_timeout_ms: 120_000,
                max_read_chars: 20_000,
                mcp_servers: Vec::new(),
            },
            cli: CliConfig {
                pre_llm_filter_enabled: false,
                permission_mode: "ReadOnly".to_string(),
            },
            workspace_root,
        }
    }

    fn apply_partial(&mut self, partial: PartialAgentConfig) {
        let mut conversation_store_overridden = false;
        let mut memory_root_overridden = false;
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
            if let Some(value) = llm.input_modalities {
                self.llm.input_modalities = normalize_input_modalities(value);
            }
            if let Some(value) = llm.temperature {
                self.llm.temperature = value;
            }
            if let Some(value) = llm.request_max_retries {
                self.llm.request_max_retries = value;
            }
            if let Some(value) = llm.stream_max_retries {
                self.llm.stream_max_retries = value;
            }
            if let Some(value) = llm.stream_idle_timeout_ms {
                self.llm.stream_idle_timeout_ms = value.max(1_000);
            }
        }

        if let Some(runtime) = partial.runtime {
            if let Some(value) = runtime.system_prompt {
                self.runtime.system_prompt = value;
            }
            if let Some(value) = runtime.max_tool_roundtrips {
                self.runtime.max_tool_roundtrips = value.map(|v| v.max(1));
            }
            if let Some(value) = runtime.data_root_dir {
                self.runtime.data_root_dir = absolutize_path(&self.workspace_root, value);
            }
            if let Some(value) = runtime.conversation_store_dir {
                conversation_store_overridden = true;
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
                    memory_root_overridden = true;
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
        if !conversation_store_overridden {
            self.runtime.conversation_store_dir = self.runtime.data_root_dir.join("conversations");
        }
        if !memory_root_overridden {
            self.runtime.memory.root_dir = self.runtime.data_root_dir.join("state").join("memory");
        }

        if let Some(tools) = partial.tools {
            if let Some(value) = tools.default_shell_timeout_ms {
                self.tools.default_shell_timeout_ms = value.max(1_000);
            }
            if let Some(value) = tools.max_read_chars {
                self.tools.max_read_chars = value.max(1_024);
            }
            if let Some(servers) = tools.mcp_servers {
                self.tools.mcp_servers = servers
                    .into_iter()
                    .filter_map(|server| build_mcp_server_config(&self.workspace_root, server))
                    .collect();
            }
        }

        if let Some(cli) = partial.cli {
            if let Some(value) = cli.pre_llm_filter_enabled {
                self.cli.pre_llm_filter_enabled = value;
            }
            if let Some(value) = cli.permission_mode
                && let Some(canonical) = normalize_permission_mode(&value)
            {
                self.cli.permission_mode = canonical.to_string();
            }
        }
    }

    fn apply_env_overrides(&mut self) {
        let mut conversation_store_overridden = false;
        let mut memory_root_overridden = false;
        if let Ok(value) = env::var("CLOUDAGENT_LLM_BASE_URL") {
            self.llm.base_url = value;
        }
        if let Ok(value) = env::var("CLOUDAGENT_LLM_API_KEY") {
            self.llm.api_key = value;
        }
        if let Ok(value) = env::var("CLOUDAGENT_LLM_MODEL") {
            self.llm.model = value;
        }
        if let Ok(value) = env::var("CLOUDAGENT_LLM_INPUT_MODALITIES") {
            self.llm.input_modalities = parse_input_modalities(&value);
        }
        if let Ok(value) = env::var("CLOUDAGENT_LLM_TEMPERATURE")
            && let Ok(parsed) = value.parse::<f32>()
        {
            self.llm.temperature = parsed;
        }
        if let Ok(value) = env::var("CLOUDAGENT_LLM_REQUEST_MAX_RETRIES")
            && let Ok(parsed) = value.parse::<u64>()
        {
            self.llm.request_max_retries = parsed;
        }
        if let Ok(value) = env::var("CLOUDAGENT_LLM_STREAM_MAX_RETRIES")
            && let Ok(parsed) = value.parse::<u64>()
        {
            self.llm.stream_max_retries = parsed;
        }
        if let Ok(value) = env::var("CLOUDAGENT_LLM_STREAM_IDLE_TIMEOUT_MS")
            && let Ok(parsed) = value.parse::<u64>()
        {
            self.llm.stream_idle_timeout_ms = parsed.max(1_000);
        }
        if let Ok(value) = env::var("CLOUDAGENT_SYSTEM_PROMPT") {
            self.runtime.system_prompt = value;
        }
        if let Ok(value) = env::var("CLOUDAGENT_MAX_TOOL_ROUNDTRIPS")
            && let Ok(parsed) = value.parse::<usize>()
        {
            self.runtime.max_tool_roundtrips = Some(parsed.max(1));
        }
        if let Ok(value) = env::var("CLOUDAGENT_CONVERSATION_STORE_DIR") {
            conversation_store_overridden = true;
            self.runtime.conversation_store_dir =
                absolutize_path(&self.workspace_root, PathBuf::from(value));
        }
        if let Ok(value) = env::var("CLOUDAGENT_DATA_ROOT_DIR") {
            self.runtime.data_root_dir =
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
        if let Ok(value) = env::var("CLOUDAGENT_MEMORY_ROOT_DIR") {
            memory_root_overridden = true;
            self.runtime.memory.root_dir =
                absolutize_path(&self.workspace_root, PathBuf::from(value));
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
        if !conversation_store_overridden {
            self.runtime.conversation_store_dir = self.runtime.data_root_dir.join("conversations");
        }
        if !memory_root_overridden {
            self.runtime.memory.root_dir = self.runtime.data_root_dir.join("state").join("memory");
        }
    }

    pub fn load_user_only(workspace_root: impl Into<PathBuf>) -> Result<Self> {
        let workspace_root = workspace_root.into();
        let mut config = Self::defaults(workspace_root.clone());
        if let Some(home) = user_home_dir() {
            let data_root = home.join(".cloudagent").join("data");
            config.runtime.data_root_dir = data_root.clone();
            config.runtime.conversation_store_dir = data_root.join("conversations");
            config.runtime.memory.root_dir = data_root.join("state").join("memory");
            let config_path = home.join(".cloudagent").join("config.toml");
            if config_path.exists() {
                let text = std::fs::read_to_string(&config_path)
                    .with_context(|| format!("failed to read {}", config_path.display()))?;
                let partial: PartialAgentConfig = toml::from_str(&text)
                    .with_context(|| format!("failed to parse {}", config_path.display()))?;
                config.apply_partial(partial);
            }
        }
        config.apply_env_overrides();
        Ok(config)
    }
}

fn normalize_input_modalities(value: Vec<InputModality>) -> Vec<InputModality> {
    let mut normalized = Vec::new();
    for modality in value {
        if !normalized.contains(&modality) {
            normalized.push(modality);
        }
    }
    if !normalized.contains(&InputModality::Text) {
        normalized.insert(0, InputModality::Text);
    }
    normalized
}

fn parse_input_modalities(value: &str) -> Vec<InputModality> {
    let parsed = value
        .split(',')
        .filter_map(|token| match token.trim().to_ascii_lowercase().as_str() {
            "" => None,
            "text" => Some(InputModality::Text),
            "image" => Some(InputModality::Image),
            _ => None,
        })
        .collect::<Vec<_>>();
    normalize_input_modalities(parsed)
}

fn config_search_paths(workspace_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = user_home_dir() {
        paths.push(home.join(".cloudagent").join("config.toml"));
    }
    paths.push(workspace_root.join(".cloudagent").join("config.toml"));
    paths.push(workspace_root.join("configs").join("config.toml"));
    paths
}

fn user_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
}

fn build_mcp_server_config(
    workspace_root: &Path,
    partial: PartialMcpServerConfig,
) -> Option<McpServerConfig> {
    let name = partial.name?.trim().to_string();
    let command = partial.command?.trim().to_string();
    if name.is_empty() || command.is_empty() {
        return None;
    }

    Some(McpServerConfig {
        name,
        command,
        args: partial.args.unwrap_or_default(),
        env: partial.env.unwrap_or_default(),
        cwd: partial
            .cwd
            .flatten()
            .map(|path| absolutize_path(workspace_root, path)),
        startup_timeout_ms: partial.startup_timeout_ms.unwrap_or(15_000).max(1_000),
        supports_parallel_tool_calls: partial.supports_parallel_tool_calls.unwrap_or(false),
        enabled: partial.enabled.unwrap_or(true),
    })
}

fn parse_memory_mode(value: &str) -> MemoryMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "basic" => MemoryMode::Basic,
        "evolve" => MemoryMode::Evolve,
        _ => MemoryMode::Off,
    }
}

fn normalize_permission_mode(value: &str) -> Option<&'static str> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("readonly") || trimmed.eq_ignore_ascii_case("safe") {
        return Some("ReadOnly");
    }
    if trimmed.eq_ignore_ascii_case("workspacewrite") || trimmed.eq_ignore_ascii_case("balanced") {
        return Some("WorkspaceWrite");
    }
    if trimmed.eq_ignore_ascii_case("fullaccess") || trimmed.eq_ignore_ascii_case("danger") {
        return Some("FullAccess");
    }
    None
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
        "Keep changes minimal, consistent, and scoped to the request, but do not default to single-keyword or single-file trial-and-error when locating code.",
        "For repository work, use this order by default: search for likely candidates, read the strongest files, then run precise commands only when needed.",
        "Treat the first repository search as a coverage pass, not a one-shot guess.",
        "On the first repository search, prefer 2-3 concise, related code clues over one bare keyword. Combine the user-visible symptom with likely code terms, behavior terms, or entrypoint terms.",
        "After the first strong search result, read 2-3 plausible files in parallel before choosing an edit path whenever the files answer independent questions.",
        "When multiple repository questions are independent, emit multiple tool calls in the same round instead of waiting for one result before requesting the next obvious file or search.",
        "Do not serialize obvious follow-up reads one file at a time when the likely candidate files are already clear.",
        "If a search returns weak results, broaden scope or reformulate the query with adjacent code terms instead of repeating another single-keyword probe.",
        "Use structured repository tools for repository discovery before falling back to exec_command; reserve commands mainly for build, test, git, and runtime verification.",
        "Prefer `search_workspace` and `read_file` for repository understanding; prefer `exec_command` mainly for validation, not for first-pass codebase discovery.",
        "When shell search is truly needed, prefer `rg` or `rg --files` over slower or noisier alternatives.",
        "Use platform-appropriate commands and workspace-relative paths unless absolute paths are explicitly required.",
        "Prefer safe, read-first workflows before mutating actions.",
        "Before modifying code or creating commits, align with the repository's existing style, conventions, and project workflow; do not make arbitrary edits or commits.",
        "Do not introduce compatibility shims, fallback patches, or risk-bearing workaround designs without explicit user alignment on a clear plan, unless the user has clearly delegated solution choice.",
        "After making changes, run the most relevant narrow validation available (tests, build, or lint) when feasible, then expand only if needed.",
    ]
    .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{InputModality, normalize_input_modalities, parse_input_modalities};

    #[test]
    fn normalize_input_modalities_keeps_text_and_deduplicates() {
        let got = normalize_input_modalities(vec![
            InputModality::Image,
            InputModality::Image,
            InputModality::Text,
        ]);

        assert_eq!(got, vec![InputModality::Image, InputModality::Text]);
    }

    #[test]
    fn parse_input_modalities_defaults_text_when_only_image_is_configured() {
        let got = parse_input_modalities("image");

        assert_eq!(got, vec![InputModality::Text, InputModality::Image]);
    }
}
