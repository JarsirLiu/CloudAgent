use agent_core::SkillRuntime;
use agent_core::approval::ApprovalGrantStoreBackend;
use agent_core::context::AgentContext;
use agent_core::host::{AgentHost, AgentHostParts, AgentMetadata};
use agent_core::model::{ChatModel, ChatModelFactory, ModelProviderSettings, ReloadableChatModel};
use agent_core::state::AgentState;
use agent_core::turn::{ExecutionPolicy, RegularTurnSettings};
use agent_memory::LongTermMemoryFacade;
use agent_model_provider::OpenAiCompatibleModel;
use agent_tools::{ToolRegistry, ToolRegistryOptions};
use anyhow::Result;
use config::AgentConfig;
use infra_store::{JsonConversationStore, RolloutRecorder};
use std::env;
use std::path::Path;
use std::sync::Arc;

struct OpenAiCompatibleModelFactory {
    template: config::LlmConfig,
}

impl ChatModelFactory for OpenAiCompatibleModelFactory {
    fn build(&self, settings: ModelProviderSettings) -> Result<Arc<dyn ChatModel>> {
        let mut config = self.template.clone();
        config.api_key = settings.api_key;
        config.base_url = settings.base_url;
        config.model = settings.model;
        Ok(Arc::new(OpenAiCompatibleModel::new(config)?))
    }
}

pub fn build_agent_host(config: AgentConfig) -> Result<Arc<AgentHost>> {
    config.validate()?;
    let context = AgentContext {
        workspace_root: config.workspace_root.clone(),
        data_root_dir: config.runtime.data_root_dir.clone(),
        conversation_store_dir: config.runtime.conversation_store_dir.clone(),
        default_shell_timeout_ms: config.tools.default_shell_timeout_ms,
        tool_output_token_limit: config.runtime.tool_output_token_limit,
    };
    let policy = ExecutionPolicy::new(config.runtime.max_tool_roundtrips);
    let regular_turn_settings = RegularTurnSettings {
        workspace_root: config.workspace_root.clone(),
        data_root_dir: config.runtime.data_root_dir.clone(),
        llm_temperature: config.llm.temperature,
        pre_llm_filter_enabled: config.cli.pre_llm_filter_enabled,
        max_tool_roundtrips: policy.max_tool_roundtrips,
        model_context_window: config.runtime.model_context_window,
        context_compaction_trigger_ratio: config.runtime.context_compaction_trigger_ratio,
        context_compaction_request_overhead_tokens: config
            .runtime
            .context_compaction_request_overhead_tokens,
        context_compaction_target_tokens: config.runtime.context_compaction_target_tokens,
        context_compaction_preserved_user_turns: config
            .runtime
            .context_compaction_preserved_user_turns,
        context_compaction_preserved_tail_tokens: config
            .runtime
            .context_compaction_preserved_tail_tokens,
        context_compaction_summary_source_tokens: config
            .runtime
            .context_compaction_summary_source_tokens,
        post_compact_token_budget: config.runtime.post_compact_token_budget,
        post_compact_memory_floor_tokens: config.runtime.post_compact_memory_floor_tokens,
        post_compact_skills_token_budget: config.runtime.post_compact_skills_token_budget,
        post_compact_mcp_token_budget: config.runtime.post_compact_mcp_token_budget,
        post_compact_max_tokens_per_memory: config.runtime.post_compact_max_tokens_per_memory,
        post_compact_max_tokens_per_skill: config.runtime.post_compact_max_tokens_per_skill,
        post_compact_max_tokens_per_mcp: config.runtime.post_compact_max_tokens_per_mcp,
        context_budget_safety_buffer_tokens: config.runtime.context_budget_safety_buffer_tokens,
        tool_output_token_limit: config.runtime.tool_output_token_limit,
        enable_skill_bucket: config.runtime.enable_skill_bucket,
        enable_mcp_bucket: config.runtime.enable_mcp_bucket,
    };
    let metadata = AgentMetadata {
        llm_model_name: config.llm.model.clone(),
        conversation_store_dir: config.runtime.conversation_store_dir.clone(),
        cli_pre_llm_filter_enabled: config.cli.pre_llm_filter_enabled,
        cli_permission_mode: config.cli.permission_mode.clone(),
        shell_name: default_shell_name(),
        system_prompt: config.runtime.system_prompt.clone(),
    };
    let model_factory = Arc::new(OpenAiCompatibleModelFactory {
        template: config.llm.clone(),
    });
    let initial_model = model_factory.build(ModelProviderSettings {
        api_key: config.llm.api_key.clone(),
        base_url: config.llm.base_url.clone(),
        model: config.llm.model.clone(),
    })?;
    let reloadable_model = Arc::new(ReloadableChatModel::new(initial_model));
    let model: Arc<dyn ChatModel> = reloadable_model.clone();
    let tools = Arc::new(ToolRegistry::with_options(
        config.tools.max_read_chars,
        ToolRegistryOptions {
            apply_patch_enabled: config.tools.apply_patch_enabled,
        },
    ));
    let store = Arc::new(JsonConversationStore::new(
        config.runtime.conversation_store_dir.clone(),
    ));
    let approval_grants: Arc<dyn ApprovalGrantStoreBackend> = store.clone();
    let rollout_recorder = Arc::new(RolloutRecorder::new(store.as_ref().clone()));
    let memory = Arc::new(LongTermMemoryFacade::new(config.runtime.memory.clone())?);
    let state = AgentState::new(metadata.system_prompt.clone());

    Ok(Arc::new(AgentHost::new(AgentHostParts {
        metadata,
        context,
        regular_turn_settings,
        policy,
        model,
        reloadable_model: Some(reloadable_model),
        model_factory: Some(model_factory),
        tools,
        state,
        store,
        approval_grants,
        rollout_recorder,
        memory,
        skills: SkillRuntime::new(
            config.runtime.skills_enabled,
            config.runtime.skill_roots.clone(),
        ),
    })))
}

fn default_shell_name() -> String {
    if cfg!(windows) {
        preferred_windows_shell().unwrap_or_else(|| "powershell".to_string())
    } else {
        "sh".to_string()
    }
}

fn preferred_windows_shell() -> Option<String> {
    for candidate in ["pwsh.exe", "pwsh", "powershell.exe", "powershell"] {
        if command_exists(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn command_exists(candidate: &str) -> bool {
    if candidate.contains('\\') || candidate.contains('/') {
        return Path::new(candidate).exists();
    }
    let Some(path_value) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_value).any(|dir| {
        let direct = dir.join(candidate);
        if direct.exists() {
            return true;
        }
        if direct.extension().is_none() {
            return dir.join(format!("{candidate}.exe")).exists();
        }
        false
    })
}
