mod engine;
mod audit;
mod observability;
mod state;
mod tasks;
mod tools;

use agent_core::{
    AgentContext, ChatModel, EnvironmentContext, ExecutionPolicy, ToolCall, ToolExecutor,
};
use agent_memory::LongTermMemoryFacade;
use agent_tools::ToolRegistry;
use anyhow::Result;
use config::AgentConfig;
use engine::{
    OpenAiCompatibleModel, emit_event, is_turn_interrupted_error, model_shell_name, next_turn_id,
    summarize_arguments,
};
use state::RuntimeState;
use state::rollout_recorder::RolloutRecorder;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use storage::JsonConversationStore;
use crate::audit::RuntimeAudit;
use crate::observability::verify_audit_chain;

pub use agent_core::ResponseItem;
pub use agent_protocol::{
    ConversationSnapshot, ConversationStatus, ConversationSummary, EventMsg, RequestId,
    ServerRequest, ServerRequestDecision, TranscriptItem, TurnItemKind, TurnState,
};

const TURN_INTERRUPTED_ERROR: &str = "turn interrupted by client";
const MANUAL_COMPACTION_MIN_HISTORY_TOKENS: usize = 20_000;

pub fn crate_name() -> &'static str {
    "agent-runtime"
}

pub struct AgentRuntime {
    config: AgentConfig,
    context: AgentContext,
    policy: ExecutionPolicy,
    model: Arc<dyn ChatModel>,
    tools: Arc<dyn ToolExecutor>,
    state: RuntimeState,
    store: JsonConversationStore,
    rollout_recorder: RolloutRecorder,
    memory: LongTermMemoryFacade,
    session_approvals: StdMutex<HashSet<String>>,
}

impl AgentRuntime {
    pub fn from_config(config: AgentConfig) -> Result<Self> {
        config.validate()?;
        let context = AgentContext {
            workspace_root: config.workspace_root.clone(),
            default_shell_timeout_ms: config.tools.default_shell_timeout_ms,
        };
        let policy = ExecutionPolicy::new(config.runtime.max_tool_roundtrips);
        let model = Arc::new(OpenAiCompatibleModel::new(config.llm.clone())?);
        let tools = Arc::new(ToolRegistry::new(config.tools.max_read_chars));
        let store = JsonConversationStore::new(config.runtime.conversation_store_dir.clone());
        let rollout_recorder = RolloutRecorder::new(store.clone());
        let memory = LongTermMemoryFacade::new(config.runtime.memory.clone())?;

        let system_prompt = config.runtime.system_prompt.clone();
        Ok(Self {
            config,
            context,
            policy,
            model,
            tools,
            state: RuntimeState::new(system_prompt),
            store,
            rollout_recorder,
            memory,
            session_approvals: StdMutex::new(HashSet::new()),
        })
    }

    pub async fn run_startup_retention_cleanup(&self) {
        let _ = self.store.prune_archived_conversations_if_needed().await;
        if let Err(err) = verify_audit_chain(&self.context.workspace_root) {
            tracing::warn!("audit chain verify failed on startup: {err:#}");
        }
    }

    pub fn llm_model_name(&self) -> &str {
        &self.config.llm.model
    }

    pub fn conversation_store_dir(&self) -> &std::path::Path {
        &self.config.runtime.conversation_store_dir
    }

    pub fn cli_pre_llm_filter_enabled(&self) -> bool {
        self.config.cli.pre_llm_filter_enabled
    }

    pub fn cli_permission_mode(&self) -> &str {
        &self.config.cli.permission_mode
    }

    pub(crate) fn environment_context(&self) -> EnvironmentContext {
        let now = chrono::Local::now();
        EnvironmentContext::new(
            self.context.workspace_root.clone(),
            model_shell_name(),
            now.format("%Y-%m-%d").to_string(),
            now.format("%H:%M:%S").to_string(),
            now.to_rfc3339(),
            now.offset().to_string(),
        )
    }

    pub(crate) fn is_tool_approved_for_session(&self, call: &ToolCall) -> bool {
        self.session_approvals
            .lock()
            .is_ok_and(|approvals| approvals.contains(&tool_approval_key(call)))
    }

    pub(crate) fn approve_tool_for_session(&self, call: &ToolCall) {
        if let Ok(mut approvals) = self.session_approvals.lock() {
            approvals.insert(tool_approval_key(call));
        }
    }
    pub(crate) fn audit(&self) -> RuntimeAudit<'_> {
        RuntimeAudit::new(self)
    }
}

fn tool_approval_key(call: &ToolCall) -> String {
    let arguments =
        serde_json::to_string(&call.arguments).unwrap_or_else(|_| call.arguments.to_string());
    format!("{}:{arguments}", call.name)
}

#[cfg(test)]
mod tests {
    use crate::engine::visible_message_count;
    use agent_core::{ConversationHistory, ToolCall};
    use serde_json::json;

    #[test]
    fn visible_message_count_excludes_system_and_tool_items() {
        let mut history = ConversationHistory::new("default", "system");
        history.push_user_message("hello");
        history.push_assistant_message(
            None,
            vec![ToolCall {
                id: "call-1".to_string(),
                name: "shell_command".to_string(),
                arguments: json!({"command": "pwd"}),
            }],
        );
        history.push_tool_result(agent_core::ToolResult {
            tool_call_id: "call-1".to_string(),
            name: "shell_command".to_string(),
            content: "D:\\work".to_string(),
            is_error: false,
            structured: None,
        });
        history.push_assistant_message(Some("done".to_string()), Vec::new());

        assert_eq!(visible_message_count(&history), 2);
    }
}

impl AgentRuntime {
    pub(crate) async fn is_turn_cancelled(&self, conversation_id: &str) -> bool {
        self.state
            .active_turn(conversation_id)
            .await
            .is_some_and(|turn| turn.is_cancelled())
    }
}

#[derive(Debug, Clone)]
pub enum ManualCompactionOutcome {
    Compacted {
        pre_context_tokens_estimate: u64,
        post_context_tokens_estimate: u64,
        pre_message_count: usize,
        post_message_count: usize,
        preserved_tail_count: usize,
    },
    Skipped {
        estimated_history_tokens: usize,
    },
}
