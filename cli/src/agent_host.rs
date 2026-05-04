use agent_core::{
    AgentContext, AgentHost, AgentHostParts, AgentMetadata, AgentState, ExecutionPolicy,
    RegularTurnSettings,
};
use agent_memory::LongTermMemoryFacade;
use agent_model_provider::OpenAiCompatibleModel;
use agent_tools::ToolRegistry;
use anyhow::Result;
use config::AgentConfig;
use infra_store::{JsonConversationStore, RolloutRecorder};
use std::env;
use std::path::Path;
use std::sync::Arc;

pub fn build_agent_host(config: AgentConfig) -> Result<Arc<AgentHost>> {
    config.validate()?;
    let context = AgentContext {
        workspace_root: config.workspace_root.clone(),
        conversation_store_dir: config.runtime.conversation_store_dir.clone(),
        default_shell_timeout_ms: config.tools.default_shell_timeout_ms,
    };
    let policy = ExecutionPolicy::new(config.runtime.max_tool_roundtrips);
    let regular_turn_settings = RegularTurnSettings {
        workspace_root: config.workspace_root.clone(),
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
    let model = Arc::new(OpenAiCompatibleModel::new(config.llm.clone())?);
    let tools = Arc::new(ToolRegistry::new(config.tools.max_read_chars));
    let store = Arc::new(JsonConversationStore::new(
        config.runtime.conversation_store_dir.clone(),
    ));
    let rollout_recorder = Arc::new(RolloutRecorder::new(store.as_ref().clone()));
    let memory = Arc::new(LongTermMemoryFacade::new(config.runtime.memory.clone())?);
    let state = AgentState::new(metadata.system_prompt.clone());

    Ok(Arc::new(AgentHost::new(AgentHostParts {
        metadata,
        context,
        regular_turn_settings,
        policy,
        model,
        tools,
        state,
        store,
        rollout_recorder,
        memory,
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
