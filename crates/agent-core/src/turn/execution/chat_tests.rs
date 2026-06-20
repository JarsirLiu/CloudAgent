use super::execute_chat_turn;
use super::{collect_discoverable_tools, compose_visible_tool_specs};
use crate::context::EnvironmentContext;
use crate::skill::{SkillRuntime, TurnSkillContext};
use crate::tool::ChatTurnToolExposure;
use crate::turn::compaction::{
    BudgetedFragmentInputs, build_budgeted_fragments_for_current_history, compaction_phase,
};
use crate::turn::compaction::{CompactionMode, maybe_compact_history};
use crate::turn::{
    AutoCompactTokenLimitScope, ChatTurnSettings, CompactionPhase, ServerRequest,
    ServerRequestDecision, ServerRequestHandler, ToolBatchOutcome, TurnHost,
};
use crate::{
    ContextFacade, ContextManager, ConversationHistory, EventMsg, FilterPolicy, ModelRequest,
    ModelResponse, ModelStreamObserver, RolloutItem, ToolCall, ToolExecutionPolicy, ToolIdentity,
    ToolSource, ToolSpec, TurnInterruptedError, TurnItemDeltaKind, TurnItemKind, TurnOutcome,
    TurnState,
};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug)]
struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("cloudagent-agent-core-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("create test workspace");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn demo_spec(name: &str) -> ToolSpec {
    ToolSpec {
        name: name.to_string(),
        identity: ToolIdentity {
            source: ToolSource::BuiltIn,
            namespace: None,
            wire_name: name.to_string(),
        },
        description: format!("demo spec for {name}"),
        parameters: serde_json::json!({"type": "object"}),
        mutating: false,
        execution_policy: ToolExecutionPolicy::Sequential,
        requires_approval: false,
        item_kind: TurnItemKind::ToolCall,
        delta_kind: TurnItemDeltaKind::ToolOutput,
        approval_reason: None,
    }
}

#[test]
fn next_round_visible_tools_include_deferred_hits_from_tool_search() {
    let default_tools = vec![demo_spec("search_workspace"), demo_spec("tool_search")];
    let deferred_tool = demo_spec("watch");
    let deferred_tool_map = BTreeMap::from([(
        deferred_tool.identity.wire_name.clone(),
        deferred_tool.clone(),
    )]);

    let visible =
        compose_visible_tool_specs(&default_tools, &deferred_tool_map, &["watch".to_string()]);

    assert_eq!(
        visible
            .iter()
            .map(|spec| spec.name.as_str())
            .collect::<Vec<_>>(),
        vec!["search_workspace", "tool_search", "watch"]
    );
}

#[test]
fn discoverable_tools_exclude_already_exposed_deferred_hits() {
    let deferred_tool_map = BTreeMap::from([
        ("watch".to_string(), demo_spec("watch")),
        ("unwatch".to_string(), demo_spec("unwatch")),
    ]);

    let discoverable = collect_discoverable_tools(&deferred_tool_map, &["watch".to_string()]);

    assert_eq!(
        discoverable
            .iter()
            .map(|spec| spec.name.as_str())
            .collect::<Vec<_>>(),
        vec!["unwatch"]
    );
}

#[test]
fn first_roundtrip_compaction_is_pre_turn_and_later_roundtrips_are_mid_turn() {
    assert_eq!(compaction_phase(0), CompactionPhase::PreTurn);
    assert_eq!(compaction_phase(1), CompactionPhase::PreTurn);
    assert_eq!(compaction_phase(2), CompactionPhase::MidTurn);
    assert_eq!(compaction_phase(4), CompactionPhase::MidTurn);
}

struct MockTurnHost {
    responses: Mutex<Vec<ModelResponse>>,
    _workspace: Arc<TestWorkspace>,
    settings: ChatTurnSettings,
    memory_fragment: Option<String>,
    last_request: Mutex<Option<ModelRequest>>,
}

impl MockTurnHost {
    fn new(responses: Vec<ModelResponse>) -> Self {
        let workspace = Arc::new(TestWorkspace::new());
        Self {
            responses: Mutex::new(responses),
            settings: ChatTurnSettings {
                workspace_root: workspace.path().to_path_buf(),
                data_root_dir: workspace.path().join("data"),
                llm_temperature: 0.0,
                pre_llm_filter_enabled: false,
                max_tool_roundtrips: Some(4),
                max_tool_only_roundtrips_after_compaction: 2,
                model_context_window: 200_000,
                model_auto_compact_token_limit: None,
                model_auto_compact_token_limit_scope: AutoCompactTokenLimitScope::Total,
                context_compaction_trigger_ratio: 0.9,
                context_compaction_request_overhead_tokens: 1_000,
                context_compaction_target_tokens: 36_000,
                context_compaction_preserved_user_turns: 3,
                context_compaction_preserved_tail_tokens: 12_000,
                context_compaction_summary_source_tokens: 24_000,
                post_compact_token_budget: 50_000,
                post_compact_memory_floor_tokens: 6_000,
                post_compact_skills_token_budget: 25_000,
                post_compact_mcp_token_budget: 8_000,
                post_compact_max_tokens_per_memory: 6_000,
                post_compact_max_tokens_per_skill: 5_000,
                post_compact_max_tokens_per_mcp: 3_000,
                context_budget_safety_buffer_tokens: 8_000,
                tool_output_token_limit: crate::ModelRequest::default_tool_output_token_limit(),
                enable_skill_bucket: false,
                enable_mcp_bucket: false,
            },
            _workspace: workspace,
            memory_fragment: None,
            last_request: Mutex::new(None),
        }
    }

    fn with_memory(mut self, memory_fragment: impl Into<String>) -> Self {
        self.memory_fragment = Some(memory_fragment.into());
        self
    }

    fn workspace_root(&self) -> &Path {
        &self.settings.workspace_root
    }

    fn last_request_messages(&self) -> Vec<crate::ResponseItem> {
        self.last_request
            .lock()
            .expect("last request lock")
            .as_ref()
            .map(|request| request.messages.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl TurnHost for MockTurnHost {
    type PermissionProfile = ();
    type ApprovalPolicy = ();

    fn chat_turn_settings(&self) -> ChatTurnSettings {
        self.settings.clone()
    }

    fn environment_context(&self) -> EnvironmentContext {
        EnvironmentContext::new(
            ".",
            "powershell",
            "2026-05-06",
            "12:00:00",
            "2026-05-06T12:00:00+08:00",
            "+08:00",
        )
    }

    fn raw_memory_fragment(&self) -> Option<String> {
        self.memory_fragment.clone()
    }

    fn skills(&self) -> SkillRuntime {
        SkillRuntime::new(true, Vec::new())
    }

    fn resolve_chat_turn_tool_exposure(
        &self,
        _permission_profile: &Self::PermissionProfile,
    ) -> ChatTurnToolExposure {
        ChatTurnToolExposure {
            default_tools: vec![],
            deferred_tools: vec![],
        }
    }

    async fn start_turn(
        &self,
        _conversation_id: String,
        _turn_id: String,
    ) -> Option<crate::state::ActiveTurnHandle> {
        unreachable!()
    }

    async fn finish_turn(&self, _conversation_id: &str) {}

    async fn is_turn_cancelled(&self, _conversation_id: &str) -> bool {
        false
    }

    fn append_conversation_event(&self, _conversation_id: &str, _event: EventMsg) {}

    async fn load_history(&self, _conversation_id: &str) -> Result<ConversationHistory> {
        unreachable!()
    }

    async fn history_from_rollout(&self, _conversation_id: &str) -> Result<ConversationHistory> {
        unreachable!()
    }

    async fn restore_turn_token_state(
        &self,
        _conversation_id: &str,
    ) -> Result<Option<crate::turn::RestoredTurnTokenState>> {
        Ok(None)
    }

    async fn save_history(&self, _history: ConversationHistory) -> Result<()> {
        Ok(())
    }

    async fn persist_rollout_items(
        &self,
        _conversation_id: &str,
        _items: &[RolloutItem],
    ) -> Result<()> {
        Ok(())
    }

    fn record_rollout_items(&self, _conversation_id: &str, _items: &[RolloutItem]) -> Result<()> {
        Ok(())
    }

    async fn flush_rollout(&self) -> Result<()> {
        Ok(())
    }

    fn should_persist_memory(&self, _history: &ConversationHistory) -> bool {
        false
    }

    fn persist_memory_from_history(&self, _history: &ConversationHistory) {}

    async fn complete_model_request(
        &self,
        cancellation_token: &CancellationToken,
        _request: ModelRequest,
    ) -> Result<ModelResponse> {
        if cancellation_token.is_cancelled() {
            return Err(anyhow::Error::new(TurnInterruptedError));
        }
        Ok(ModelResponse {
            content: Some(
                "Current Task:\n- Continue the active coding task.\nProgress:\n- Compacted prior context.\nKey Decisions:\n- Keep recent conversation tail.\nImportant Context:\n- Preserve the latest raw messages.\nTool / Code Facts:\n- None.\nNext Steps:\n- Continue the current turn."
                    .to_string(),
            ),
            reasoning: None,
            tool_calls: Vec::new(),
            finish_reason: None,
            model_name: Some("test-model".to_string()),
            usage: None,
        })
    }

    async fn complete_model_request_streaming(
        &self,
        _cancellation_token: &CancellationToken,
        request: ModelRequest,
        observer: &mut dyn ModelStreamObserver,
    ) -> Result<ModelResponse> {
        *self.last_request.lock().expect("last request lock") = Some(request);
        let response = self.responses.lock().expect("responses lock").remove(0);
        if let Some(reasoning) = response.reasoning.clone() {
            observer.on_reasoning_delta(crate::model::ReasoningDelta::Text {
                content_index: 0,
                delta: reasoning,
            });
        }
        if let Some(content) = response.content.clone() {
            observer.on_text_delta(content);
        }
        Ok(response)
    }

    async fn run_tool_batch(
        &self,
        _conversation_id: &str,
        _turn_id: &str,
        _permission_profile: &Self::PermissionProfile,
        _approval_policy: &Self::ApprovalPolicy,
        _cancellation_token: CancellationToken,
        _tool_calls: Vec<ToolCall>,
        _tool_specs: &[ToolSpec],
        _discoverable_tools: &[ToolSpec],
        _context_manager: &mut ContextManager,
        _events: &mut Vec<EventMsg>,
        _on_event: &mut (dyn for<'a> FnMut(&'a EventMsg) + Send + '_),
        _approval: &dyn ServerRequestHandler,
        _denied_requests: &mut HashSet<String>,
    ) -> Result<ToolBatchOutcome> {
        Ok(ToolBatchOutcome {
            cancelled: false,
            exposed_tools: Vec::new(),
        })
    }

    fn audit_turn_started(&self, _conversation_id: &str, _user_input: &[crate::InputItem]) {}
    fn audit_turn_completed(
        &self,
        _conversation_id: &str,
        _turn_id: &str,
        _state: &str,
        _events_count: usize,
        _model_name: Option<&str>,
    ) {
    }
    fn audit_turn_cancelled(&self, _conversation_id: &str, _turn_id: &str, _reason: &str) {}
    fn audit_turn_failed(&self, _conversation_id: &str, _turn_id: &str, _error: &str) {}
    fn audit_model_request_started(
        &self,
        _conversation_id: &str,
        _turn_id: &str,
        _message_count: usize,
        _tool_count: usize,
    ) {
    }
    fn audit_model_response_received(
        &self,
        _conversation_id: &str,
        _turn_id: &str,
        _model_name: Option<&str>,
        _has_content: bool,
        _tool_call_count: usize,
    ) {
    }
}

#[tokio::test]
async fn reasoning_item_ids_advance_across_tool_roundtrips() {
    let host = MockTurnHost::new(vec![
        ModelResponse {
            content: None,
            reasoning: Some("first reasoning".to_string()),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                identity: ToolIdentity::built_in("read_file"),
                arguments: json!({"path":"README.md"}),
            }],
            finish_reason: None,
            model_name: Some("test-model".to_string()),
            usage: None,
        },
        ModelResponse {
            content: Some("final answer".to_string()),
            reasoning: Some("second reasoning".to_string()),
            tool_calls: vec![],
            finish_reason: None,
            model_name: Some("test-model".to_string()),
            usage: None,
        },
    ]);

    let history = ConversationHistory::new("default".to_string(), "system".to_string());
    let mut delivered = Vec::new();
    let outcome: TurnOutcome = execute_chat_turn(
        &host,
        "default",
        "turn-1",
        &(),
        &(),
        CancellationToken::new(),
        history,
        &mut |event| delivered.push(event.clone()),
        &(|_req: ServerRequest| async move {
            Ok(ServerRequestDecision::accept(Some("ok".to_string())))
        }),
    )
    .await
    .expect("turn outcome");

    let reasoning_starts = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            EventMsg::ItemStarted {
                kind: TurnItemKind::Reasoning,
                item_id,
                ..
            } => Some(item_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        reasoning_starts,
        vec![
            "reasoning:turn-1:0".to_string(),
            "reasoning:turn-1:1".to_string()
        ]
    );
    assert!(
        delivered.iter().any(
            |event| matches!(event, EventMsg::TurnCompleted { turn_id } if turn_id == "turn-1")
        )
    );
}

#[tokio::test]
async fn finish_reason_tool_use_keeps_turn_open_for_next_round() {
    let host = MockTurnHost::new(vec![
        ModelResponse {
            content: Some("intermediate text".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
            finish_reason: Some("tool_calls".to_string()),
            model_name: Some("test-model".to_string()),
            usage: None,
        },
        ModelResponse {
            content: Some("final answer".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
            finish_reason: Some("stop".to_string()),
            model_name: Some("test-model".to_string()),
            usage: None,
        },
    ]);

    let history = ConversationHistory::new("default".to_string(), "system".to_string());
    let mut delivered = Vec::new();
    let outcome: TurnOutcome = execute_chat_turn(
        &host,
        "default",
        "turn-1",
        &(),
        &(),
        CancellationToken::new(),
        history,
        &mut |event| delivered.push(event.clone()),
        &(|_req: ServerRequest| async move {
            Ok(ServerRequestDecision::accept(Some("ok".to_string())))
        }),
    )
    .await
    .expect("turn outcome");

    assert!(matches!(outcome.state, TurnState::Completed));
    assert!(
        delivered.iter().any(
            |event| matches!(event, EventMsg::TurnCompleted { turn_id } if turn_id == "turn-1")
        )
    );
    assert!(
        host.responses.lock().expect("responses lock").is_empty(),
        "turn should have consumed both model responses"
    );
}

#[tokio::test]
async fn automatic_compaction_preserves_explicit_continuation_mode() {
    let host = MockTurnHost::new(vec![]);
    let mut history = ConversationHistory::new("default".to_string(), "system".to_string());
    for index in 0..8 {
        history.push_user_message(crate::text_input_items(format!(
            "historic user message {index} {}",
            "x".repeat(4_000)
        )));
        history.push_assistant_message(
            Some(format!(
                "historic assistant reply {index} {}",
                "y".repeat(4_000)
            )),
            None,
            Vec::new(),
        );
    }

    let pre_turn = maybe_compact_history(
        &host,
        &mut history.clone(),
        &CancellationToken::new(),
        CompactionMode::Automatic {
            estimated_total_tokens: usize::MAX,
            token_limit_reached: true,
            phase: CompactionPhase::PreTurn,
        },
    )
    .await
    .expect("pre-turn compaction result")
    .expect("pre-turn compaction applied");
    assert_eq!(pre_turn.phase, CompactionPhase::PreTurn);

    let mid_turn = maybe_compact_history(
        &host,
        &mut history,
        &CancellationToken::new(),
        CompactionMode::Automatic {
            estimated_total_tokens: usize::MAX,
            token_limit_reached: true,
            phase: CompactionPhase::MidTurn,
        },
    )
    .await
    .expect("mid-turn compaction result")
    .expect("mid-turn compaction applied");
    assert_eq!(mid_turn.phase, CompactionPhase::MidTurn);
}

#[tokio::test]
async fn interrupted_compaction_leaves_history_unchanged() {
    let host = MockTurnHost::new(vec![]);
    let mut history = ConversationHistory::new("default".to_string(), "system".to_string());
    for index in 0..4 {
        history.push_user_message(crate::text_input_items(format!(
            "historic user message {index} {}",
            "x".repeat(4_000)
        )));
        history.push_assistant_message(
            Some(format!(
                "historic assistant reply {index} {}",
                "y".repeat(4_000)
            )),
            None,
            Vec::new(),
        );
    }
    let original_messages = history.messages.clone();
    let cancellation_token = CancellationToken::new();
    cancellation_token.cancel();

    let err = maybe_compact_history(
        &host,
        &mut history,
        &cancellation_token,
        CompactionMode::Automatic {
            estimated_total_tokens: usize::MAX,
            token_limit_reached: true,
            phase: CompactionPhase::MidTurn,
        },
    )
    .await
    .expect_err("cancelled compaction should error");

    assert!(err.downcast_ref::<TurnInterruptedError>().is_some());
    assert_eq!(history.messages.len(), original_messages.len());
    assert_eq!(
        format!("{:?}", history.messages),
        format!("{:?}", original_messages)
    );
}

#[tokio::test]
async fn explicit_skill_mentions_inject_skill_instructions_into_model_request() {
    let host = MockTurnHost::new(vec![ModelResponse {
        content: Some("done".to_string()),
        reasoning: None,
        tool_calls: Vec::new(),
        finish_reason: None,
        model_name: Some("test-model".to_string()),
        usage: None,
    }]);
    let skill_dir = host
        .workspace_root()
        .join(".cloudagent")
        .join("skills")
        .join("repo-reader");
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: repo-reader\ndescription: Read repository structure\npolicy:\n  allow_implicit_invocation: true\ndependencies:\n  tools: [rg, git]\n---\n\n# Repo Reader\nUse this skill for repository analysis.\n",
    )
    .expect("write skill file");

    let mut history = ConversationHistory::new("default".to_string(), "system".to_string());
    history.push_user_message(crate::text_input_items("please use $repo-reader"));
    let mut delivered = Vec::new();
    let outcome = execute_chat_turn(
        &host,
        "default",
        "turn-1",
        &(),
        &(),
        CancellationToken::new(),
        history,
        &mut |event| delivered.push(event.clone()),
        &(|_req: ServerRequest| async move {
            Ok(ServerRequestDecision::accept(Some("ok".to_string())))
        }),
    )
    .await
    .expect("turn outcome");

    assert!(matches!(outcome.state, TurnState::Completed));
    let rendered_messages = host
        .last_request_messages()
        .into_iter()
        .filter_map(|message| match message {
            crate::ResponseItem::User { content } => {
                Some(crate::input_items_to_plain_text(&content))
            }
            crate::ResponseItem::System { content } => Some(content),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered_messages.contains("<skill>\n<name>repo-reader</name>"));
    assert!(rendered_messages.contains("<path>"));
    assert!(rendered_messages.contains("Use this skill for repository analysis."));
    assert!(
        delivered.iter().any(
            |event| matches!(event, EventMsg::TurnCompleted { turn_id } if turn_id == "turn-1")
        )
    );
}

#[tokio::test]
async fn skill_injection_does_not_carry_across_turns_without_remention() {
    let host = MockTurnHost::new(vec![
        ModelResponse {
            content: Some("done".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
            finish_reason: None,
            model_name: Some("test-model".to_string()),
            usage: None,
        },
        ModelResponse {
            content: Some("done again".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
            finish_reason: None,
            model_name: Some("test-model".to_string()),
            usage: None,
        },
    ]);
    let skill_dir = host
        .workspace_root()
        .join(".cloudagent")
        .join("skills")
        .join("repo-reader");
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: repo-reader\ndescription: Read repository structure\npolicy:\n  allow_implicit_invocation: true\ndependencies:\n  tools: [rg]\n---\n\n# Repo Reader\nUse this skill for repository analysis.\n",
    )
    .expect("write skill file");

    let mut history = ConversationHistory::new("default".to_string(), "system".to_string());
    history.push_user_message(crate::text_input_items("please use $repo-reader"));
    let mut delivered = Vec::new();
    let first = execute_chat_turn(
        &host,
        "default",
        "turn-1",
        &(),
        &(),
        CancellationToken::new(),
        history,
        &mut |event| delivered.push(event.clone()),
        &(|_req: ServerRequest| async move {
            Ok(ServerRequestDecision::accept(Some("ok".to_string())))
        }),
    )
    .await
    .expect("first turn outcome");
    assert!(matches!(first.state, TurnState::Completed));
    let first_rendered = host
        .last_request_messages()
        .into_iter()
        .filter_map(|message| match message {
            crate::ResponseItem::User { content } => {
                Some(crate::input_items_to_plain_text(&content))
            }
            crate::ResponseItem::System { content } => Some(content),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(first_rendered.contains("<skill>\n<name>repo-reader</name>"));

    let mut second_history = first.history.clone();
    second_history.push_user_message(crate::text_input_items("continue with the summary"));
    let second = execute_chat_turn(
        &host,
        "default",
        "turn-2",
        &(),
        &(),
        CancellationToken::new(),
        second_history,
        &mut |event| delivered.push(event.clone()),
        &(|_req: ServerRequest| async move {
            Ok(ServerRequestDecision::accept(Some("ok".to_string())))
        }),
    )
    .await
    .expect("second turn outcome");
    assert!(matches!(second.state, TurnState::Completed));
    let second_rendered = host
        .last_request_messages()
        .into_iter()
        .filter_map(|message| match message {
            crate::ResponseItem::User { content } => {
                Some(crate::input_items_to_plain_text(&content))
            }
            crate::ResponseItem::System { content } => Some(content),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!second_rendered.contains("<skill>\n<name>repo-reader</name>"));
}

#[test]
fn recomputing_budgeted_fragments_after_compaction_can_restore_memory_context() {
    let host = MockTurnHost::new(vec![]).with_memory("remember this important long-term fact");
    let settings = {
        let mut settings = host.chat_turn_settings();
        settings.model_context_window = 90_000;
        settings.context_compaction_trigger_ratio = 0.05;
        settings.context_compaction_request_overhead_tokens = 0;
        settings.context_compaction_target_tokens = 4_000;
        settings.context_compaction_preserved_tail_tokens = 2_000;
        settings.post_compact_token_budget = 2_000;
        settings.post_compact_memory_floor_tokens = 500;
        settings.post_compact_max_tokens_per_memory = 500;
        settings.context_budget_safety_buffer_tokens = 0;
        settings
    };
    let environment = host.environment_context();
    let filter_policy = FilterPolicy { enabled: false };

    let mut context_manager =
        ContextManager::from_history(ConversationHistory::new("default", "system"));
    context_manager
        .history_mut()
        .push_user_message(crate::text_input_items("hello"));
    context_manager.history_mut().push_assistant_message(
        Some("A".repeat(24_000)),
        None,
        Vec::new(),
    );
    let before = build_budgeted_fragments_for_current_history(
        &ContextFacade::new(),
        &context_manager,
        filter_policy,
        &environment,
        &settings,
        BudgetedFragmentInputs {
            raw_memory_fragment: host.raw_memory_fragment(),
            turn_skill_context: TurnSkillContext::default(),
        },
    );
    assert!(!before.fragments.iter().any(|item| {
        matches!(item, crate::ResponseItem::User { content } if crate::input_items_to_plain_text(content).contains("<long_term_memory>"))
    }));

    let mut compacted_history = ConversationHistory::new("default".to_string(), "system");
    compacted_history.push_user_message(crate::text_input_items("hello"));
    context_manager = ContextManager::from_history(compacted_history);
    let after = build_budgeted_fragments_for_current_history(
        &ContextFacade::new(),
        &context_manager,
        filter_policy,
        &environment,
        &settings,
        BudgetedFragmentInputs {
            raw_memory_fragment: host.raw_memory_fragment(),
            turn_skill_context: TurnSkillContext::default(),
        },
    );
    assert!(after.fragments.iter().any(|item| {
        matches!(item, crate::ResponseItem::User { content } if crate::input_items_to_plain_text(content).contains("<long_term_memory>"))
    }));
}
