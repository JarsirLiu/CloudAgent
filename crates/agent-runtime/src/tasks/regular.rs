use super::{RuntimeTask, TaskContext, TaskKind};
use crate::tools::ToolBatchRunner;
use crate::{AgentRuntime, emit_event};
use crate::observability::{ContextBudgetLogEntry, append_context_budget_log};
use agent_core::{
    ContextCompactionConfig, ContextFragment, ContextManager, ContextFacade, ConversationHistory,
    ModelUsage, RolloutItem,
};
use agent_core::context::MemoryBudgetSource;
use agent_protocol::{
    ApprovalPolicy, EventMsg, PermissionProfile, ServerRequest, ServerRequestDecision, TranscriptItem, TurnItemDeltaKind,
    TurnItemKind, TurnState,
};
use anyhow::Result;
use std::collections::HashSet;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub(crate) struct TurnOutcome {
    pub(crate) turn_id: String,
    pub(crate) events: Vec<EventMsg>,
    pub(crate) history: ConversationHistory,
    pub(crate) model_name: Option<String>,
    pub(crate) state: TurnState,
}

pub(crate) struct RegularTurnTask;

impl<E, F, Fut> RuntimeTask<E, F, Fut> for RegularTurnTask
where
    E: FnMut(&EventMsg) + Send,
    F: Fn(ServerRequest) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
{
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }

    async fn run(
        self,
        ctx: TaskContext<'_, E>,
        history: ConversationHistory,
        approval: F,
    ) -> Result<TurnOutcome> {
        execute_regular_turn(
            ctx.runtime,
            ctx.conversation_id,
            ctx.turn_id,
            ctx.permission_profile,
            ctx.approval_policy,
            ctx.cancellation_token,
            history,
            ctx.on_event,
            approval,
        )
        .await
    }
}

pub(crate) async fn execute_regular_turn<E, F, Fut>(
    runtime: &AgentRuntime,
    conversation_id: &str,
    turn_id: &str,
    permission_profile: &PermissionProfile,
    approval_policy: &ApprovalPolicy,
    cancellation_token: CancellationToken,
    history: ConversationHistory,
    on_event: &mut E,
    approval: F,
) -> Result<TurnOutcome>
where
    E: FnMut(&EventMsg) + Send,
    F: Fn(ServerRequest) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
{
    let mut context_manager = ContextManager::from_history(history);
    let mut events = Vec::new();
    let mut last_model_name = None;
    let mut assistant_item_seq: usize = 0;
    let tool_specs = runtime
        .tools
        .specs_for_context("explore", "repository_analysis");
    let mut denied_requests = HashSet::new();
    let environment_context = runtime.environment_context();
    let raw_memory_fragment = runtime
        .memory
        .build_load_plan()
        .ok()
        .and_then(|p| p.inject_prefix)
        .filter(|s| !s.trim().is_empty());
    let mut turn_total_usage = ModelUsage::default();
    let context_facade = ContextFacade::new();
    let mut last_sdk_context_tokens: Option<usize> = None;
    let mut history_len_at_last_sdk_usage: Option<usize> = None;

    let mut roundtrip_count = 0usize;
    loop {
        if let Some(limit) = runtime.policy.max_tool_roundtrips
            && roundtrip_count >= limit
        {
            break;
        }
        roundtrip_count += 1;
        if cancellation_token.is_cancelled() || runtime.is_turn_cancelled(conversation_id).await {
            emit_event(
                &mut events,
                on_event,
                EventMsg::TurnCancelled {
                    turn_id: turn_id.to_string(),
                    reason: "interrupted by client".to_string(),
                },
            );
            return Ok(TurnOutcome {
                turn_id: turn_id.to_string(),
                events,
                history: context_manager.history().clone(),
                model_name: last_model_name,
                state: TurnState::Cancelled,
            });
        }

        let compaction_config = ContextCompactionConfig {
            model_context_window: runtime.config.runtime.model_context_window,
            trigger_ratio: runtime.config.runtime.context_compaction_trigger_ratio,
            request_overhead_tokens: context_facade.estimate_request_overhead_tokens(
                &context_manager.history().messages,
                &environment_context.render(),
                &tool_specs,
                runtime
                    .config
                    .runtime
                    .context_compaction_request_overhead_tokens,
            ),
            compacted_target_tokens: runtime.config.runtime.context_compaction_target_tokens,
            preserved_user_turns: runtime
                .config
                .runtime
                .context_compaction_preserved_user_turns,
            preserved_tail_tokens: runtime
                .config
                .runtime
                .context_compaction_preserved_tail_tokens,
            summary_source_max_tokens: runtime
                .config
                .runtime
                .context_compaction_summary_source_tokens,
        };

        let estimated_total_tokens = if let Some(sdk_tokens) = last_sdk_context_tokens {
            let delta_start = history_len_at_last_sdk_usage
                .unwrap_or_else(|| context_manager.history().messages.len())
                .min(context_manager.history().messages.len());
            let local_delta_tokens = context_facade.estimate_history_tokens_for_compaction(
                &context_manager.history().messages[delta_start..],
                &runtime.config.workspace_root,
            );
            sdk_tokens.saturating_add(local_delta_tokens)
        } else {
            context_facade.estimate_history_tokens_for_compaction(
                &context_manager.history().messages,
                &runtime.config.workspace_root,
            )
        };

        let budgeted = context_facade.build_memory_budgeted_fragments(
            &context_manager.history().messages,
            environment_context.render(),
            &tool_specs,
            &runtime.config.workspace_root,
            runtime.config.runtime.model_context_window,
            runtime.config.runtime.context_compaction_trigger_ratio,
            runtime
                .config
                .runtime
                .context_compaction_request_overhead_tokens,
            MemoryBudgetSource {
                memory: raw_memory_fragment.clone(),
                skills: None,
                mcp: None,
                enable_skills_bucket: runtime.config.runtime.enable_skill_bucket,
                enable_mcp_bucket: runtime.config.runtime.enable_mcp_bucket,
                post_compact_budget_tokens: runtime.config.runtime.post_compact_token_budget,
                post_compact_memory_floor_tokens: runtime
                    .config
                    .runtime
                    .post_compact_memory_floor_tokens,
                post_compact_skills_budget_tokens: runtime
                    .config
                    .runtime
                    .post_compact_skills_token_budget,
                post_compact_mcp_budget_tokens: runtime.config.runtime.post_compact_mcp_token_budget,
                post_compact_max_tokens_per_memory: runtime
                    .config
                    .runtime
                    .post_compact_max_tokens_per_memory,
                post_compact_max_tokens_per_skill: runtime
                    .config
                    .runtime
                    .post_compact_max_tokens_per_skill,
                post_compact_max_tokens_per_mcp: runtime
                    .config
                    .runtime
                    .post_compact_max_tokens_per_mcp,
                safety_buffer_tokens: runtime
                    .config
                    .runtime
                    .context_budget_safety_buffer_tokens,
            },
        );

        let prepared = context_facade
            .prepare_model_request(
                &mut context_manager,
                &runtime.config.workspace_root,
                budgeted.fragments,
                tool_specs.clone(),
                runtime.config.llm.temperature,
                compaction_config,
                estimated_total_tokens,
                runtime.config.runtime.post_compact_memory_floor_tokens,
                runtime.config.runtime.context_budget_safety_buffer_tokens,
                |summary_request| async {
                    let response = runtime
                        .complete_model_request(&cancellation_token, summary_request)
                        .await?;
                    Ok(response.content)
                },
            )
            .await?;
        let history_tokens_now = context_facade.estimate_history_tokens_for_compaction(
            &context_manager.history().messages,
            &runtime.config.workspace_root,
        );
        let trigger_tokens = ((runtime.config.runtime.model_context_window as f32)
            * runtime.config.runtime.context_compaction_trigger_ratio)
            as usize;
        let overhead_now = compaction_config.request_overhead_tokens;
        let available_history_tokens = trigger_tokens
            .saturating_sub(overhead_now)
            .saturating_sub(runtime.config.runtime.post_compact_memory_floor_tokens)
            .saturating_sub(runtime.config.runtime.context_budget_safety_buffer_tokens)
            .max(1);
        let compaction_triggered_now = estimated_total_tokens > available_history_tokens;
        let _ = append_context_budget_log(
            &runtime.config.workspace_root,
            &ContextBudgetLogEntry {
                conversation_id: conversation_id.to_string(),
                turn_id: turn_id.to_string(),
                model_context_window: runtime.config.runtime.model_context_window,
                trigger_ratio: runtime.config.runtime.context_compaction_trigger_ratio,
                trigger_tokens,
                estimated_total_tokens,
                sdk_total_tokens: last_sdk_context_tokens,
                history_tokens: history_tokens_now,
                overhead_tokens: overhead_now,
                memory_floor_tokens: runtime.config.runtime.post_compact_memory_floor_tokens,
                safety_buffer_tokens: runtime.config.runtime.context_budget_safety_buffer_tokens,
                compaction_triggered: compaction_triggered_now,
                hard_cap_triggered: budgeted.audit.hard_cap_triggered,
                memory_before: budgeted.audit.memory_before,
                memory_after: budgeted.audit.memory_after,
                skills_before: budgeted.audit.skills_before,
                skills_after: budgeted.audit.skills_after,
                mcp_before: budgeted.audit.mcp_before,
                mcp_after: budgeted.audit.mcp_after,
            },
        );
        if prepared.compaction_requested {
            emit_event(
                &mut events,
                on_event,
                EventMsg::ContextCompactionStarted {
                    turn_id: turn_id.to_string(),
                    estimated_tokens: estimated_total_tokens as u64,
                },
            );
        }
        if let Some(compacted) = &prepared.compaction {
            runtime
                .persist_rollout_items(
                    conversation_id,
                    &[RolloutItem::Compacted {
                        summary: compacted.summary.clone(),
                        rendered_summary: compacted.rendered_summary.clone(),
                        replacement_history: compacted.replacement_history.clone(),
                    }],
                )
                .await?;
            runtime
                .state
                .save_history(context_manager.history().clone())
                .await;
            emit_event(
                &mut events,
                on_event,
                EventMsg::ContextCompacted {
                    turn_id: turn_id.to_string(),
                    pre_context_tokens_estimate: compacted.pre_context_tokens_estimate,
                    post_context_tokens_estimate: compacted.post_context_tokens_estimate,
                    pre_message_count: compacted.pre_message_count,
                    post_message_count: compacted.post_message_count,
                    preserved_tail_count: compacted.post_message_count.saturating_sub(2),
                },
            );
        }

        let model_request = prepared.model_request;

        emit_event(
            &mut events,
            on_event,
            EventMsg::ModelRequestStarted {
                turn_id: turn_id.to_string(),
                message_count: model_request.messages.len(),
                tool_count: tool_specs.len(),
            },
        );
        runtime.audit().model_request_started(
            conversation_id,
            turn_id,
            model_request.messages.len(),
            tool_specs.len(),
        );

        let mut streaming_assistant_item_id: Option<String> = None;
        let response = runtime
            .complete_model_request_streaming(
                &cancellation_token,
                model_request,
                &mut |delta: String| {
                    if delta.is_empty() {
                        return;
                    }
                    let item_id = streaming_assistant_item_id.get_or_insert_with(|| {
                        let id = format!("assistant:{turn_id}:{}", assistant_item_seq);
                        assistant_item_seq += 1;
                        emit_event(
                            &mut events,
                            on_event,
                            EventMsg::ItemStarted {
                                turn_id: turn_id.to_string(),
                                item_id: id.clone(),
                                kind: TurnItemKind::AssistantMessage,
                                title: Some("assistant_message".to_string()),
                            },
                        );
                        id
                    });
                    emit_event(
                        &mut events,
                        on_event,
                        EventMsg::ItemDelta {
                            turn_id: turn_id.to_string(),
                            item_id: item_id.clone(),
                            kind: TurnItemDeltaKind::Text,
                            delta,
                        },
                    );
                },
            )
            .await?;

        let had_streaming_assistant_item = streaming_assistant_item_id.is_some();
        if let Some(item_id) = streaming_assistant_item_id.take() {
            emit_event(
                &mut events,
                on_event,
                EventMsg::ItemCompleted {
                    turn_id: turn_id.to_string(),
                    item_id: item_id.clone(),
                    item: TranscriptItem::AgentMessage {
                        id: item_id,
                        text: response.content.clone().unwrap_or_default(),
                    },
                },
            );
        }

        last_model_name = response.model_name.clone();
        let tool_calls = response.tool_calls.clone();
        if !had_streaming_assistant_item
            && let Some(content) = response.content.clone()
            && !content.trim().is_empty()
        {
            emit_assistant_item(
                &mut events,
                on_event,
                turn_id,
                &content,
                &mut assistant_item_seq,
            );
        }
        emit_event(
            &mut events,
            on_event,
            EventMsg::ModelResponseReceived {
                turn_id: turn_id.to_string(),
                model_name: response.model_name.clone(),
                has_content: response.content.is_some(),
                tool_call_count: tool_calls.len(),
            },
        );
        runtime.audit().model_response_received(
            conversation_id,
            turn_id,
            response.model_name.as_deref(),
            response.content.is_some(),
            tool_calls.len(),
        );
        if let Some(usage) = response.usage.clone() {
            turn_total_usage.add_assign(&usage);
            last_sdk_context_tokens = Some(usage.total_tokens as usize);
            emit_event(
                &mut events,
                on_event,
                EventMsg::TokenUsageUpdated {
                    turn_id: turn_id.to_string(),
                    last_usage: usage,
                    total_usage: turn_total_usage.clone(),
                    model_context_window: Some(runtime.config.runtime.model_context_window),
                },
            );
        }

        let assistant_response_item =
            context_manager.record_assistant_message(response.content.clone(), tool_calls.clone());
        history_len_at_last_sdk_usage = Some(context_manager.history().messages.len());
        runtime
            .persist_rollout_items(
                conversation_id,
                &[RolloutItem::from(assistant_response_item)],
            )
            .await?;
        runtime
            .state
            .save_history(context_manager.history().clone())
            .await;

        if tool_calls.is_empty() {
            if !had_streaming_assistant_item && response.content.is_none() {
                emit_assistant_item(
                    &mut events,
                    on_event,
                    turn_id,
                    "The model returned an empty response.",
                    &mut assistant_item_seq,
                );
            }
            emit_event(
                &mut events,
                on_event,
                EventMsg::TurnCompleted {
                    turn_id: turn_id.to_string(),
                },
            );
            return Ok(TurnOutcome {
                turn_id: turn_id.to_string(),
                events,
                history: context_manager.history().clone(),
                model_name: last_model_name,
                state: TurnState::Completed,
            });
        }

        let tool_batch = ToolBatchRunner::new(
            runtime,
            conversation_id,
            turn_id,
            permission_profile,
            approval_policy,
            cancellation_token.clone(),
            &tool_specs,
        )
        .run(
            tool_calls,
            &mut context_manager,
            &mut events,
            on_event,
            &approval,
            &mut denied_requests,
        )
        .await?;
        if tool_batch.cancelled {
            return Ok(TurnOutcome {
                turn_id: turn_id.to_string(),
                events,
                history: context_manager.history().clone(),
                model_name: last_model_name,
                state: TurnState::Cancelled,
            });
        }
    }

    let roundtrip_limit_message =
        "Reached the configured tool roundtrip limit before the model produced a final answer."
            .to_string();
    emit_assistant_item(
        &mut events,
        on_event,
        turn_id,
        &roundtrip_limit_message,
        &mut assistant_item_seq,
    );
    let roundtrip_limit_item =
        context_manager.record_assistant_message(Some(roundtrip_limit_message), Vec::new());
    runtime
        .persist_rollout_items(conversation_id, &[RolloutItem::from(roundtrip_limit_item)])
        .await?;
    runtime
        .state
        .save_history(context_manager.history().clone())
        .await;
    emit_event(
        &mut events,
        on_event,
        EventMsg::TurnCompleted {
            turn_id: turn_id.to_string(),
        },
    );
    Ok(TurnOutcome {
        turn_id: turn_id.to_string(),
        events,
        history: context_manager.history().clone(),
        model_name: last_model_name,
        state: TurnState::Completed,
    })
}

fn emit_assistant_item(
    events: &mut Vec<EventMsg>,
    on_event: &mut impl FnMut(&EventMsg),
    turn_id: &str,
    content: &str,
    assistant_item_seq: &mut usize,
) {
    let assistant_item_id = format!("assistant:{turn_id}:{}", *assistant_item_seq);
    *assistant_item_seq += 1;
    emit_event(
        events,
        on_event,
        EventMsg::ItemStarted {
            turn_id: turn_id.to_string(),
            item_id: assistant_item_id.clone(),
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        },
    );
    emit_event(
        events,
        on_event,
        EventMsg::ItemDelta {
            turn_id: turn_id.to_string(),
            item_id: assistant_item_id.clone(),
            kind: TurnItemDeltaKind::Text,
            delta: content.to_string(),
        },
    );
    emit_event(
        events,
        on_event,
        EventMsg::ItemCompleted {
            turn_id: turn_id.to_string(),
            item_id: assistant_item_id.clone(),
            item: TranscriptItem::AgentMessage {
                id: assistant_item_id,
                text: content.to_string(),
            },
        },
    );
}
