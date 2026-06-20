use super::loop_guard::LoopGuard;
use super::token_usage::RestoredTurnTokenState;
use super::{AutoCompactWindow, RequestTokenBaseline, TurnHost};
use crate::skill::{render_skill_catalog, render_skill_injection};
use crate::{ContextFacade, ContextManager, EventMsg, FilterPolicy};
use anyhow::Result;
use std::collections::{BTreeMap, HashSet};

pub(super) struct ChatTurnState {
    pub(super) context_manager: ContextManager,
    pub(super) events: Vec<EventMsg>,
    pub(super) last_model_name: Option<String>,
    pub(super) assistant_item_seq: usize,
    pub(super) deferred_tool_map: BTreeMap<String, crate::ToolSpec>,
    pub(super) exposed_tool_names: Vec<String>,
    pub(super) denied_requests: HashSet<String>,
    pub(super) loop_guard: LoopGuard,
    pub(super) skill_summary: Option<String>,
    pub(super) turn_explicit_skill_fragments: Vec<crate::ResponseItem>,
    pub(super) context_facade: ContextFacade,
    pub(super) filter_policy: FilterPolicy,
    pub(super) token_usage_state: crate::TokenUsageState,
    pub(super) request_baseline: RequestTokenBaseline,
    pub(super) auto_compact_window: AutoCompactWindow,
    pub(super) reasoning_item_seq: usize,
}

impl ChatTurnState {
    pub(super) async fn new<H: TurnHost>(
        host: &H,
        conversation_id: &str,
        permission_profile: &H::PermissionProfile,
        history: crate::ConversationHistory,
    ) -> Result<Self> {
        let settings = host.chat_turn_settings();
        let context_manager = ContextManager::from_history(history);
        let tool_exposure = host.resolve_chat_turn_tool_exposure(permission_profile);
        let deferred_tool_map = tool_exposure
            .deferred_tools
            .iter()
            .cloned()
            .map(|spec| (spec.identity.wire_name.clone(), spec))
            .collect::<BTreeMap<_, _>>();
        let skill_runtime = host.skills();
        let skill_catalog = skill_runtime.load_catalog(&settings.workspace_root);
        let skill_summary =
            render_skill_catalog(&skill_catalog.skills_allowed_for_implicit_invocation());
        // Skill bodies are turn-scoped. We only inject explicitly selected skill
        // documents for the latest user message, and we re-evaluate on every turn.
        let turn_explicit_skill_fragments = skill_runtime
            .collect_turn_explicit_skill_documents(
                &context_manager.history().messages,
                &skill_catalog,
            )
            .into_iter()
            .map(|document| render_skill_injection(&document))
            .collect::<Vec<_>>();
        let restored_token_state =
            restore_turn_token_state_from_host(host, conversation_id).await?;

        Ok(Self {
            context_manager,
            events: Vec::new(),
            last_model_name: None,
            assistant_item_seq: 0,
            deferred_tool_map,
            exposed_tool_names: Vec::new(),
            denied_requests: HashSet::new(),
            loop_guard: LoopGuard::new(),
            skill_summary,
            turn_explicit_skill_fragments,
            context_facade: ContextFacade::new(),
            filter_policy: FilterPolicy {
                enabled: settings.pre_llm_filter_enabled,
            },
            token_usage_state: restored_token_state.usage,
            request_baseline: restored_token_state.request_baseline,
            auto_compact_window: AutoCompactWindow::from_snapshot(
                restored_token_state.auto_compact_window,
            ),
            reasoning_item_seq: 0,
        })
    }
}

async fn restore_turn_token_state_from_host<H: TurnHost>(
    host: &H,
    conversation_id: &str,
) -> Result<RestoredTurnTokenState> {
    Ok(host
        .restore_turn_token_state(conversation_id)
        .await?
        .unwrap_or_default())
}
