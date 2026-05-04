use super::loop_guard::LoopGuard;
use super::{ServerRequestHandler, ToolBatchOutcome, TurnHost, TurnOutcome};
use crate::context::{ContextFragment, MemoryBudgetSource};
use crate::{
    ContextBudgetLogEntry, ContextCompactionConfig, ContextFacade, ContextManager, FilterPolicy,
    ModelStreamObserver, ModelUsage, RolloutItem, append_context_budget_log,
    emit_assistant_message_item, emit_event,
};
use crate::{EventMsg, TranscriptItem, TurnItemDeltaKind, TurnItemKind, TurnState};
use anyhow::Result;
use std::collections::HashSet;
use tokio_util::sync::CancellationToken;

pub async fn execute_regular_turn<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    turn_id: &str,
    permission_profile: &H::PermissionProfile,
    approval_policy: &H::ApprovalPolicy,
    cancellation_token: CancellationToken,
    history: crate::ConversationHistory,
    on_event: &mut (dyn for<'a> FnMut(&'a EventMsg) + Send + '_),
    approval: &dyn ServerRequestHandler,
) -> Result<TurnOutcome> {
    let settings = host.regular_turn_settings();
    let mut context_manager = ContextManager::from_history(history);
    let mut events = Vec::new();
    let mut last_model_name = None;
    let mut assistant_item_seq: usize = 0;
    let tool_specs = host.resolve_regular_turn_tools(permission_profile);
    let mut denied_requests = HashSet::new();
    let mut loop_guard = LoopGuard::new();
    let environment_context = host.environment_context();
    let raw_memory_fragment = host.raw_memory_fragment();
    let mut turn_total_usage = ModelUsage::default();
    let context_facade = ContextFacade::new();
    let filter_policy = FilterPolicy {
        enabled: settings.pre_llm_filter_enabled,
    };
    let mut last_sdk_context_tokens: Option<usize> = None;
    let mut history_len_at_last_sdk_usage: Option<usize> = None;

    let mut roundtrip_count = 0usize;
    loop {
        if let Some(limit) = settings.max_tool_roundtrips
            && roundtrip_count >= limit
        {
            break;
        }
        roundtrip_count += 1;
        if cancellation_token.is_cancelled() || host.is_turn_cancelled(conversation_id).await {
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
            model_context_window: settings.model_context_window,
            trigger_ratio: settings.context_compaction_trigger_ratio,
            request_overhead_tokens: context_facade.estimate_request_overhead_tokens(
                &context_manager.history().messages,
                &environment_context.render(),
                &tool_specs,
                settings.context_compaction_request_overhead_tokens,
            ),
            compacted_target_tokens: settings.context_compaction_target_tokens,
            preserved_user_turns: settings.context_compaction_preserved_user_turns,
            preserved_tail_tokens: settings.context_compaction_preserved_tail_tokens,
            summary_source_max_tokens: settings.context_compaction_summary_source_tokens,
        };

        let estimated_total_tokens = if let Some(sdk_tokens) = last_sdk_context_tokens {
            let delta_start = history_len_at_last_sdk_usage
                .unwrap_or_else(|| context_manager.history().messages.len())
                .min(context_manager.history().messages.len());
            let local_delta_tokens = context_facade.estimate_history_tokens_for_compaction(
                &context_manager.history().messages[delta_start..],
                filter_policy,
                &settings.workspace_root,
            );
            sdk_tokens.saturating_add(local_delta_tokens)
        } else {
            context_facade.estimate_history_tokens_for_compaction(
                &context_manager.history().messages,
                filter_policy,
                &settings.workspace_root,
            )
        };
        let compaction_estimated_total_tokens = context_facade
            .estimate_history_tokens_for_canonical_compaction(
                &context_manager.history().messages,
                &settings.workspace_root,
            );

        let budgeted = context_facade.build_memory_budgeted_fragments(
            &context_manager.history().messages,
            filter_policy,
            environment_context.render(),
            &tool_specs,
            &settings.workspace_root,
            settings.model_context_window,
            settings.context_compaction_trigger_ratio,
            settings.context_compaction_request_overhead_tokens,
            MemoryBudgetSource {
                memory: raw_memory_fragment.clone(),
                skills: None,
                mcp: None,
                enable_skills_bucket: settings.enable_skill_bucket,
                enable_mcp_bucket: settings.enable_mcp_bucket,
                post_compact_budget_tokens: settings.post_compact_token_budget,
                post_compact_memory_floor_tokens: settings.post_compact_memory_floor_tokens,
                post_compact_skills_budget_tokens: settings.post_compact_skills_token_budget,
                post_compact_mcp_budget_tokens: settings.post_compact_mcp_token_budget,
                post_compact_max_tokens_per_memory: settings.post_compact_max_tokens_per_memory,
                post_compact_max_tokens_per_skill: settings.post_compact_max_tokens_per_skill,
                post_compact_max_tokens_per_mcp: settings.post_compact_max_tokens_per_mcp,
                safety_buffer_tokens: settings.context_budget_safety_buffer_tokens,
            },
        );

        let prepared = context_facade
            .prepare_model_request(
                &mut context_manager,
                &settings.workspace_root,
                filter_policy,
                budgeted.fragments,
                tool_specs.clone(),
                settings.llm_temperature,
                compaction_config,
                estimated_total_tokens.max(compaction_estimated_total_tokens),
                settings.post_compact_memory_floor_tokens,
                settings.context_budget_safety_buffer_tokens,
                |summary_request| async {
                    let response = host
                        .complete_model_request(&cancellation_token, summary_request)
                        .await?;
                    Ok(response.content)
                },
            )
            .await?;
        let history_tokens_now = context_facade.estimate_history_tokens_for_compaction(
            &context_manager.history().messages,
            filter_policy,
            &settings.workspace_root,
        );
        let trigger_tokens = ((settings.model_context_window as f32)
            * settings.context_compaction_trigger_ratio) as usize;
        let overhead_now = compaction_config.request_overhead_tokens;
        let available_history_tokens = trigger_tokens
            .saturating_sub(overhead_now)
            .saturating_sub(settings.post_compact_memory_floor_tokens)
            .saturating_sub(settings.context_budget_safety_buffer_tokens)
            .max(1);
        let compaction_triggered_now = estimated_total_tokens > available_history_tokens;
        let _ = append_context_budget_log(
            &settings.workspace_root,
            &ContextBudgetLogEntry {
                conversation_id: conversation_id.to_string(),
                turn_id: turn_id.to_string(),
                model_context_window: settings.model_context_window,
                trigger_ratio: settings.context_compaction_trigger_ratio,
                trigger_tokens,
                estimated_total_tokens,
                filter_enabled: settings.pre_llm_filter_enabled,
                sdk_total_tokens: last_sdk_context_tokens,
                history_tokens: history_tokens_now,
                overhead_tokens: overhead_now,
                memory_floor_tokens: settings.post_compact_memory_floor_tokens,
                safety_buffer_tokens: settings.context_budget_safety_buffer_tokens,
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
        let persistent_compaction = prepared
            .compaction
            .as_ref()
            .filter(|compaction| !compaction.ephemeral);
        if persistent_compaction.is_some() {
            emit_event(
                &mut events,
                on_event,
                EventMsg::ContextCompactionStarted {
                    turn_id: turn_id.to_string(),
                    estimated_tokens: estimated_total_tokens as u64,
                },
            );
        }
        if let Some(compacted) = persistent_compaction {
            host.persist_rollout_items(
                conversation_id,
                &[RolloutItem::Compacted {
                    summary: compacted.summary.clone(),
                    rendered_summary: compacted.rendered_summary.clone(),
                    replacement_history: compacted.replacement_history.clone(),
                }],
            )
            .await?;
            host.save_history(context_manager.history().clone()).await?;
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
        host.audit_model_request_started(
            conversation_id,
            turn_id,
            model_request.messages.len(),
            tool_specs.len(),
        );

        struct TurnStreamObserver<'a, E: FnMut(&EventMsg) + ?Sized> {
            turn_id: &'a str,
            assistant_item_seq: &'a mut usize,
            streaming_assistant_item_id: &'a mut Option<String>,
            events: &'a mut Vec<EventMsg>,
            on_event: &'a mut E,
        }

        impl<E: FnMut(&EventMsg) + Send + ?Sized> ModelStreamObserver for TurnStreamObserver<'_, E> {
            fn on_text_delta(&mut self, delta: String) {
                if delta.is_empty() {
                    return;
                }
                let item_id = self.streaming_assistant_item_id.get_or_insert_with(|| {
                    let id = format!("assistant:{}:{}", self.turn_id, *self.assistant_item_seq);
                    *self.assistant_item_seq += 1;
                    emit_event(
                        self.events,
                        self.on_event,
                        EventMsg::ItemStarted {
                            turn_id: self.turn_id.to_string(),
                            item_id: id.clone(),
                            kind: TurnItemKind::AssistantMessage,
                            title: Some("assistant_message".to_string()),
                        },
                    );
                    id
                });
                emit_event(
                    self.events,
                    self.on_event,
                    EventMsg::ItemDelta {
                        turn_id: self.turn_id.to_string(),
                        item_id: item_id.clone(),
                        kind: TurnItemDeltaKind::Text,
                        delta,
                    },
                );
            }

            fn on_retry(
                &mut self,
                stage: crate::ModelRetryStage,
                attempt: u64,
                delay: std::time::Duration,
            ) {
                emit_event(
                    self.events,
                    self.on_event,
                    EventMsg::ModelRetrying {
                        turn_id: self.turn_id.to_string(),
                        stage,
                        attempt,
                        next_delay_ms: delay.as_millis().try_into().unwrap_or(u64::MAX),
                    },
                );
            }
        }

        let mut streaming_assistant_item_id: Option<String> = None;
        let mut stream_observer = TurnStreamObserver {
            turn_id,
            assistant_item_seq: &mut assistant_item_seq,
            streaming_assistant_item_id: &mut streaming_assistant_item_id,
            events: &mut events,
            on_event,
        };
        let response = host
            .complete_model_request_streaming(
                &cancellation_token,
                model_request,
                &mut stream_observer,
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
            emit_assistant_message_item(
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
        host.audit_model_response_received(
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
                    model_context_window: Some(settings.model_context_window),
                },
            );
        }

        let assistant_response_item =
            context_manager.record_assistant_message(response.content.clone(), tool_calls.clone());
        history_len_at_last_sdk_usage = Some(context_manager.history().messages.len());
        host.persist_rollout_items(
            conversation_id,
            &[RolloutItem::from(assistant_response_item)],
        )
        .await?;
        host.save_history(context_manager.history().clone()).await?;

        if tool_calls.is_empty() {
            loop_guard.reset();
            if !had_streaming_assistant_item && response.content.is_none() {
                emit_assistant_message_item(
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

        let tool_batch: ToolBatchOutcome = host
            .run_tool_batch(
                conversation_id,
                turn_id,
                permission_profile,
                approval_policy,
                cancellation_token.clone(),
                tool_calls.clone(),
                &tool_specs,
                &mut context_manager,
                &mut events,
                on_event,
                approval,
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
        if let Some(loop_abort) = loop_guard.record_roundtrip(
            &tool_calls,
            &tool_specs,
            &context_manager.history().messages,
        ) {
            let message = format!(
                "Stopped this turn because the model entered a repetitive loop: the same read-only tool calls and results repeated {} times without progress.",
                loop_abort.repeated_count
            );
            emit_assistant_message_item(
                &mut events,
                on_event,
                turn_id,
                &message,
                &mut assistant_item_seq,
            );
            let failed_item =
                context_manager.record_assistant_message(Some(message.clone()), Vec::new());
            host.persist_rollout_items(conversation_id, &[RolloutItem::from(failed_item)])
                .await?;
            host.save_history(context_manager.history().clone()).await?;
            emit_event(
                &mut events,
                on_event,
                EventMsg::TurnFailed {
                    turn_id: turn_id.to_string(),
                    error: message,
                },
            );
            return Ok(TurnOutcome {
                turn_id: turn_id.to_string(),
                events,
                history: context_manager.history().clone(),
                model_name: last_model_name,
                state: TurnState::Failed,
            });
        }
    }

    let roundtrip_limit_message =
        "Reached the configured tool roundtrip limit before the model produced a final answer."
            .to_string();
    emit_assistant_message_item(
        &mut events,
        on_event,
        turn_id,
        &roundtrip_limit_message,
        &mut assistant_item_seq,
    );
    let roundtrip_limit_item =
        context_manager.record_assistant_message(Some(roundtrip_limit_message), Vec::new());
    host.persist_rollout_items(conversation_id, &[RolloutItem::from(roundtrip_limit_item)])
        .await?;
    host.save_history(context_manager.history().clone()).await?;
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
