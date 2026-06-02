use super::compaction::{CompactionContinuation, CompactionMode, maybe_compact_history};
use super::loop_guard::LoopGuard;
use super::{ServerRequestHandler, ToolBatchOutcome, TurnHost, TurnOutcome};
use crate::context::{ContextFragment, ContextInjectionStrategy, MemoryBudgetSource};
use crate::model::ReasoningDelta;
use crate::skill::{render_skill_catalog, render_skill_injection};
use crate::{
    ContextBudgetLogEntry, ContextFacade, ContextManager, FilterPolicy, ModelStreamObserver,
    ModelUsage, RolloutItem, append_context_budget_log, emit_assistant_message_item, emit_event,
};
use crate::{EventMsg, TranscriptItem, TurnItemDeltaKind, TurnItemKind, TurnState};
use anyhow::Result;
use std::collections::{BTreeMap, HashSet};
use tokio_util::sync::CancellationToken;

fn apply_signed_delta(base: usize, current: usize, previous: usize) -> usize {
    if current >= previous {
        base.saturating_add(current - previous)
    } else {
        base.saturating_sub(previous - current)
    }
}

fn finish_reason_implies_tool_use(finish_reason: Option<&str>) -> bool {
    matches!(finish_reason, Some("tool_calls") | Some("tool_use"))
}

async fn restore_budget_baseline_from_host<H: TurnHost>(
    host: &H,
    conversation_id: &str,
) -> Result<(Option<usize>, Option<usize>)> {
    let restored = host.restore_budget_baseline(conversation_id).await?;
    Ok(match restored {
        Some(baseline) => (
            Some(baseline.sdk_total_tokens),
            Some(baseline.request_estimated_tokens),
        ),
        None => (None, None),
    })
}

#[allow(clippy::too_many_arguments)]
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
    let tool_exposure = host.resolve_regular_turn_tool_exposure(permission_profile);
    let deferred_tool_map = tool_exposure
        .deferred_tools
        .iter()
        .cloned()
        .map(|spec| (spec.identity.wire_name.clone(), spec))
        .collect::<BTreeMap<_, _>>();
    let mut exposed_tool_names = Vec::new();
    let mut denied_requests = HashSet::new();
    let mut loop_guard = LoopGuard::new();
    let environment_context = host.environment_context();
    let raw_memory_fragment = host.raw_memory_fragment();
    let skill_runtime = host.skills();
    let skill_catalog = skill_runtime.load_catalog(&settings.workspace_root);
    let skill_summary =
        render_skill_catalog(&skill_catalog.skills_allowed_for_implicit_invocation());
    // Skill bodies are turn-scoped. We only inject explicitly selected skill
    // documents for the latest user message, and we re-evaluate on every turn.
    let turn_explicit_skill_fragments = skill_runtime
        .collect_turn_explicit_skill_documents(&context_manager.history().messages, &skill_catalog)
        .into_iter()
        .map(|document| render_skill_injection(&document))
        .collect::<Vec<_>>();
    let mut turn_total_usage = ModelUsage::default();
    let context_facade = ContextFacade::new();
    let filter_policy = FilterPolicy {
        enabled: settings.pre_llm_filter_enabled,
    };
    let (mut last_sdk_context_tokens, mut last_request_estimated_tokens) =
        restore_budget_baseline_from_host(host, conversation_id).await?;
    let mut reasoning_item_seq = 0usize;

    let mut roundtrip_count = 0usize;
    loop {
        if let Some(limit) = settings.max_tool_roundtrips
            && roundtrip_count >= limit
        {
            break;
        }
        roundtrip_count += 1;
        let tool_specs = compose_visible_tool_specs(
            &tool_exposure.default_tools,
            &deferred_tool_map,
            &exposed_tool_names,
        );
        let discoverable_tools =
            collect_discoverable_tools(&deferred_tool_map, &exposed_tool_names);
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

        let budgeted_before_compaction = build_budgeted_fragments_for_current_history(
            &context_facade,
            &context_manager,
            filter_policy,
            &environment_context,
            &tool_specs,
            &settings,
            BudgetedFragmentInputs {
                raw_memory_fragment: raw_memory_fragment.clone(),
                skill_summary: skill_summary.clone(),
            },
        );
        let candidate_fragments = append_rendered_fragments(
            budgeted_before_compaction.fragments.clone(),
            &turn_explicit_skill_fragments,
        );
        let pre_compaction_injection_strategy = ContextInjectionStrategy::Standard;
        let mut candidate_request = context_manager
            .build_current_model_request_with_rendered_fragments(
                &candidate_fragments,
                pre_compaction_injection_strategy,
                tool_specs.clone(),
                settings.llm_temperature,
            );
        candidate_request.tool_output_token_limit = settings.tool_output_token_limit;
        candidate_request.messages = context_facade.apply_pre_llm_filter(
            candidate_request.messages,
            filter_policy,
            &settings.workspace_root,
        );
        let candidate_request_tokens =
            context_facade.estimate_model_request_tokens(&candidate_request);
        let estimated_total_tokens = match (last_sdk_context_tokens, last_request_estimated_tokens)
        {
            (Some(sdk_tokens), Some(previous_request_tokens)) => apply_signed_delta(
                sdk_tokens,
                candidate_request_tokens,
                previous_request_tokens,
            ),
            _ => candidate_request_tokens,
        };
        let compaction_estimated_total_tokens = context_facade
            .estimate_history_tokens_for_canonical_compaction(
                &context_manager.history().messages,
                &settings.workspace_root,
            );

        let compaction = maybe_compact_history(
            host,
            context_manager.history_mut(),
            &cancellation_token,
            CompactionMode::Automatic {
                estimated_total_tokens: estimated_total_tokens
                    .max(compaction_estimated_total_tokens),
                memory_floor_tokens: settings.post_compact_memory_floor_tokens,
                safety_buffer_tokens: settings.context_budget_safety_buffer_tokens,
                continuation: compaction_continuation(roundtrip_count),
            },
        )
        .await?;
        if compaction.is_some() {
            emit_event(
                &mut events,
                on_event,
                EventMsg::ContextCompactionStarted {
                    turn_id: turn_id.to_string(),
                    continuation: compaction
                        .as_ref()
                        .map(|compacted| compacted.continuation)
                        .unwrap_or(CompactionContinuation::PreTurn),
                    estimated_tokens: estimated_total_tokens as u64,
                },
            );
        }
        if let Some(compacted) = compaction.as_ref() {
            host.persist_rollout_items(
                conversation_id,
                &[RolloutItem::Compacted {
                    summary: compacted.summary.clone(),
                    rendered_summary: compacted.rendered_summary.clone(),
                    continuation: compacted.continuation,
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
                    continuation: compacted.continuation,
                    pre_context_tokens_estimate: compacted.pre_context_tokens_estimate,
                    post_context_tokens_estimate: compacted.post_context_tokens_estimate,
                    pre_message_count: compacted.pre_message_count,
                    post_message_count: compacted.post_message_count,
                    preserved_tail_count: compacted.post_message_count.saturating_sub(2),
                },
            );
        }
        let budgeted = if compaction.is_some() {
            build_budgeted_fragments_for_current_history(
                &context_facade,
                &context_manager,
                filter_policy,
                &environment_context,
                &tool_specs,
                &settings,
                BudgetedFragmentInputs {
                    raw_memory_fragment: raw_memory_fragment.clone(),
                    skill_summary: skill_summary.clone(),
                },
            )
        } else {
            budgeted_before_compaction
        };
        let prepared_fragments =
            append_rendered_fragments(budgeted.fragments.clone(), &turn_explicit_skill_fragments);
        let injection_strategy = compaction
            .as_ref()
            .map(|compacted| match compacted.continuation {
                CompactionContinuation::PreTurn => ContextInjectionStrategy::Standard,
                CompactionContinuation::MidTurn => {
                    ContextInjectionStrategy::MidTurnCompactionContinuation
                }
            })
            .unwrap_or(ContextInjectionStrategy::Standard);
        let model_request = context_facade
            .prepare_model_request(
                &context_manager,
                &settings.workspace_root,
                filter_policy,
                prepared_fragments,
                injection_strategy,
                tool_specs.clone(),
                settings.llm_temperature,
            )
            .model_request;
        let mut model_request = model_request;
        model_request.tool_output_token_limit = settings.tool_output_token_limit;
        let final_budget = context_facade.check_final_model_request_budget(
            &model_request,
            settings.model_context_window as usize,
            settings.context_budget_safety_buffer_tokens,
        );
        let history_tokens_now = context_facade.estimate_history_tokens_for_compaction(
            &context_manager.history().messages,
            filter_policy,
            &settings.workspace_root,
        );
        let trigger_tokens = ((settings.model_context_window as f32)
            * settings.context_compaction_trigger_ratio) as usize;
        let overhead_now = context_facade.estimate_request_overhead_tokens(
            &context_manager.history().messages,
            &environment_context.render(),
            &tool_specs,
            settings.context_compaction_request_overhead_tokens,
        );
        let available_history_tokens = trigger_tokens
            .saturating_sub(overhead_now)
            .saturating_sub(settings.post_compact_memory_floor_tokens)
            .saturating_sub(settings.context_budget_safety_buffer_tokens)
            .max(1);
        let compaction_triggered_now = estimated_total_tokens > available_history_tokens;
        let _ = append_context_budget_log(
            &settings.data_root_dir,
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
                hard_cap_triggered: budgeted.audit.hard_cap_triggered || final_budget.exceeded,
                memory_before: budgeted.audit.memory_before,
                memory_after: budgeted.audit.memory_after,
                skills_before: budgeted.audit.skills_before,
                skills_after: budgeted.audit.skills_after,
                mcp_before: budgeted.audit.mcp_before,
                mcp_after: budgeted.audit.mcp_after,
            },
        );
        if final_budget.exceeded {
            let message = format!(
                "Stopped before sending the model request because the final input context exceeded the budget (estimated {} tokens > limit {}). Narrow the request context or strengthen input filtering before retrying.",
                final_budget.estimated_tokens, final_budget.limit_tokens
            );
            emit_assistant_message_item(
                &mut events,
                on_event,
                turn_id,
                &message,
                &mut assistant_item_seq,
            );
            let failed_item =
                context_manager.record_assistant_message(Some(message.clone()), None, Vec::new());
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
            reasoning_item_seq: &'a mut usize,
            streaming_reasoning_item_id: &'a mut Option<String>,
            reasoning_text_buffer: &'a mut String,
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
                            call_id: None,
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
                        call_id: None,
                        kind: TurnItemDeltaKind::Text,
                        segment_index: None,
                        delta,
                    },
                );
            }

            fn on_reasoning_delta(&mut self, delta: ReasoningDelta) {
                let (kind, segment_index, delta): (TurnItemDeltaKind, Option<usize>, String) =
                    match delta {
                        ReasoningDelta::SummaryText {
                            summary_index,
                            delta,
                        } => (
                            TurnItemDeltaKind::ReasoningSummary,
                            Some(summary_index),
                            delta,
                        ),
                        ReasoningDelta::Text {
                            content_index,
                            delta,
                        } => (TurnItemDeltaKind::ReasoningText, Some(content_index), delta),
                    };
                if delta.is_empty() {
                    return;
                }
                let item_id = self.streaming_reasoning_item_id.get_or_insert_with(|| {
                    let id = format!("reasoning:{}:{}", self.turn_id, *self.reasoning_item_seq);
                    *self.reasoning_item_seq += 1;
                    emit_event(
                        self.events,
                        self.on_event,
                        EventMsg::ItemStarted {
                            turn_id: self.turn_id.to_string(),
                            item_id: id.clone(),
                            call_id: None,
                            kind: TurnItemKind::Reasoning,
                            title: Some("reasoning".to_string()),
                        },
                    );
                    id
                });
                self.reasoning_text_buffer.push_str(&delta);
                emit_event(
                    self.events,
                    self.on_event,
                    EventMsg::ItemDelta {
                        turn_id: self.turn_id.to_string(),
                        item_id: item_id.clone(),
                        call_id: None,
                        kind,
                        segment_index,
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
        let mut streaming_reasoning_item_id: Option<String> = None;
        let mut reasoning_text_buffer = String::new();
        let mut stream_observer = TurnStreamObserver {
            turn_id,
            assistant_item_seq: &mut assistant_item_seq,
            streaming_assistant_item_id: &mut streaming_assistant_item_id,
            reasoning_item_seq: &mut reasoning_item_seq,
            streaming_reasoning_item_id: &mut streaming_reasoning_item_id,
            reasoning_text_buffer: &mut reasoning_text_buffer,
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
                    call_id: None,
                    item: TranscriptItem::AgentMessage {
                        id: item_id,
                        text: response.content.clone().unwrap_or_default(),
                    },
                },
            );
        }
        if let Some(item_id) = streaming_reasoning_item_id.take() {
            emit_event(
                &mut events,
                on_event,
                EventMsg::ItemCompleted {
                    turn_id: turn_id.to_string(),
                    item_id: item_id.clone(),
                    call_id: None,
                    item: TranscriptItem::Reasoning {
                        id: item_id,
                        title: "reasoning".to_string(),
                        text: response
                            .reasoning
                            .clone()
                            .unwrap_or_else(|| reasoning_text_buffer.clone()),
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
            last_request_estimated_tokens = Some(final_budget.estimated_tokens);
            emit_event(
                &mut events,
                on_event,
                EventMsg::TokenUsageUpdated {
                    turn_id: turn_id.to_string(),
                    last_usage: usage,
                    total_usage: turn_total_usage.clone(),
                    model_context_window: Some(settings.model_context_window),
                    request_estimated_tokens: final_budget.estimated_tokens as u64,
                },
            );
        }

        let assistant_response_item = context_manager.record_assistant_message(
            response.content.clone(),
            response.reasoning.clone(),
            tool_calls.clone(),
        );
        host.persist_rollout_items(
            conversation_id,
            &[RolloutItem::from(assistant_response_item)],
        )
        .await?;
        host.save_history(context_manager.history().clone()).await?;

        let finish_reason = response.finish_reason.as_deref();
        if tool_calls.is_empty() && !finish_reason_implies_tool_use(finish_reason) {
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

        if tool_calls.is_empty() && finish_reason_implies_tool_use(finish_reason) {
            loop_guard.reset();
            continue;
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
                &discoverable_tools,
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
        exposed_tool_names = tool_batch.exposed_tools;
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
                context_manager.record_assistant_message(Some(message.clone()), None, Vec::new());
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
        context_manager.record_assistant_message(Some(roundtrip_limit_message), None, Vec::new());
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

fn compaction_continuation(roundtrip_count: usize) -> CompactionContinuation {
    if roundtrip_count <= 1 {
        CompactionContinuation::PreTurn
    } else {
        CompactionContinuation::MidTurn
    }
}

struct BudgetedFragmentInputs {
    raw_memory_fragment: Option<String>,
    skill_summary: Option<String>,
}

fn build_budgeted_fragments_for_current_history(
    context_facade: &ContextFacade,
    context_manager: &ContextManager,
    filter_policy: FilterPolicy,
    environment_context: &crate::context::EnvironmentContext,
    tool_specs: &[crate::ToolSpec],
    settings: &crate::turn::RegularTurnSettings,
    inputs: BudgetedFragmentInputs,
) -> crate::context::BudgetedFragments {
    context_facade.build_memory_budgeted_fragments(
        &context_manager.history().messages,
        filter_policy,
        environment_context.render(),
        tool_specs,
        &settings.workspace_root,
        settings.model_context_window,
        settings.context_compaction_trigger_ratio,
        settings.context_compaction_request_overhead_tokens,
        MemoryBudgetSource {
            memory: inputs.raw_memory_fragment,
            skills: inputs.skill_summary,
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
    )
}

fn append_rendered_fragments(
    mut fragments: Vec<crate::ResponseItem>,
    extra_fragments: &[crate::ResponseItem],
) -> Vec<crate::ResponseItem> {
    fragments.extend(extra_fragments.iter().cloned());
    fragments
}

fn compose_visible_tool_specs(
    default_tools: &[crate::ToolSpec],
    deferred_tool_map: &BTreeMap<String, crate::ToolSpec>,
    exposed_tool_names: &[String],
) -> Vec<crate::ToolSpec> {
    let mut tools = default_tools.to_vec();
    for tool_name in exposed_tool_names {
        if let Some(spec) = deferred_tool_map.get(tool_name)
            && !tools
                .iter()
                .any(|existing| existing.identity.wire_name == spec.identity.wire_name)
        {
            tools.push(spec.clone());
        }
    }
    tools
}

fn collect_discoverable_tools(
    deferred_tool_map: &BTreeMap<String, crate::ToolSpec>,
    exposed_tool_names: &[String],
) -> Vec<crate::ToolSpec> {
    deferred_tool_map
        .iter()
        .filter(|(tool_name, _)| !exposed_tool_names.iter().any(|name| name == *tool_name))
        .map(|(_, spec)| spec.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::build_budgeted_fragments_for_current_history;
    use super::compaction_continuation;
    use super::execute_regular_turn;
    use super::{BudgetedFragmentInputs, collect_discoverable_tools, compose_visible_tool_specs};
    use crate::context::EnvironmentContext;
    use crate::skill::SkillRuntime;
    use crate::tool::RegularTurnToolExposure;
    use crate::turn::compaction::{CompactionContinuation, CompactionMode, maybe_compact_history};
    use crate::turn::{
        RegularTurnSettings, ServerRequest, ServerRequestDecision, ServerRequestHandler,
        ToolBatchOutcome, TurnHost,
    };
    use crate::{
        ContextFacade, ContextManager, ConversationHistory, EventMsg, FilterPolicy, ModelRequest,
        ModelResponse, ModelStreamObserver, ModelUsage, RolloutItem, ToolCall, ToolExecutionPolicy,
        ToolIdentity, ToolSource, ToolSpec, TurnItemDeltaKind, TurnItemKind, TurnOutcome,
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
    fn latest_rollout_token_usage_event_carries_request_estimate() {
        let items = [
            RolloutItem::from(EventMsg::TokenUsageUpdated {
                turn_id: "turn-1".to_string(),
                last_usage: ModelUsage {
                    total_tokens: 120,
                    ..ModelUsage::default()
                },
                total_usage: ModelUsage {
                    total_tokens: 120,
                    ..ModelUsage::default()
                },
                model_context_window: Some(200_000),
                request_estimated_tokens: 110,
            }),
            RolloutItem::from(EventMsg::TokenUsageUpdated {
                turn_id: "turn-2".to_string(),
                last_usage: ModelUsage {
                    total_tokens: 240,
                    ..ModelUsage::default()
                },
                total_usage: ModelUsage {
                    total_tokens: 360,
                    ..ModelUsage::default()
                },
                model_context_window: Some(200_000),
                request_estimated_tokens: 222,
            }),
        ];

        let restored = items.iter().rev().find_map(|item| match item {
            RolloutItem::EventMsg {
                event:
                    EventMsg::TokenUsageUpdated {
                        last_usage,
                        request_estimated_tokens,
                        ..
                    },
            } => Some((last_usage.total_tokens, *request_estimated_tokens)),
            _ => None,
        });

        assert_eq!(restored, Some((240, 222)));
    }

    #[test]
    fn first_roundtrip_compaction_is_pre_turn_and_later_roundtrips_are_mid_turn() {
        assert_eq!(compaction_continuation(0), CompactionContinuation::PreTurn);
        assert_eq!(compaction_continuation(1), CompactionContinuation::PreTurn);
        assert_eq!(compaction_continuation(2), CompactionContinuation::MidTurn);
        assert_eq!(compaction_continuation(4), CompactionContinuation::MidTurn);
    }

    struct MockTurnHost {
        responses: Mutex<Vec<ModelResponse>>,
        _workspace: Arc<TestWorkspace>,
        settings: RegularTurnSettings,
        memory_fragment: Option<String>,
        last_request: Mutex<Option<ModelRequest>>,
    }

    impl MockTurnHost {
        fn new(responses: Vec<ModelResponse>) -> Self {
            let workspace = Arc::new(TestWorkspace::new());
            Self {
                responses: Mutex::new(responses),
                settings: RegularTurnSettings {
                    workspace_root: workspace.path().to_path_buf(),
                    data_root_dir: workspace.path().join("data"),
                    llm_temperature: 0.0,
                    pre_llm_filter_enabled: false,
                    max_tool_roundtrips: Some(4),
                    model_context_window: 200_000,
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

        fn turn_interrupted_error(&self) -> &'static str {
            "interrupted"
        }

        fn regular_turn_settings(&self) -> RegularTurnSettings {
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

        fn resolve_regular_turn_tool_exposure(
            &self,
            _permission_profile: &Self::PermissionProfile,
        ) -> RegularTurnToolExposure {
            RegularTurnToolExposure {
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

        async fn history_from_rollout(
            &self,
            _conversation_id: &str,
        ) -> Result<ConversationHistory> {
            unreachable!()
        }

        async fn restore_budget_baseline(
            &self,
            _conversation_id: &str,
        ) -> Result<Option<crate::turn::RestoredBudgetBaseline>> {
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

        fn record_rollout_items(
            &self,
            _conversation_id: &str,
            _items: &[RolloutItem],
        ) -> Result<()> {
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
                anyhow::bail!(self.turn_interrupted_error());
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
        let outcome: TurnOutcome = execute_regular_turn(
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
        assert!(delivered.iter().any(
            |event| matches!(event, EventMsg::TurnCompleted { turn_id } if turn_id == "turn-1")
        ));
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
        let outcome: TurnOutcome = execute_regular_turn(
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
        assert!(delivered.iter().any(
            |event| matches!(event, EventMsg::TurnCompleted { turn_id } if turn_id == "turn-1")
        ));
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
                memory_floor_tokens: 0,
                safety_buffer_tokens: 0,
                continuation: CompactionContinuation::PreTurn,
            },
        )
        .await
        .expect("pre-turn compaction result")
        .expect("pre-turn compaction applied");
        assert_eq!(pre_turn.continuation, CompactionContinuation::PreTurn);

        let mid_turn = maybe_compact_history(
            &host,
            &mut history,
            &CancellationToken::new(),
            CompactionMode::Automatic {
                estimated_total_tokens: usize::MAX,
                memory_floor_tokens: 0,
                safety_buffer_tokens: 0,
                continuation: CompactionContinuation::MidTurn,
            },
        )
        .await
        .expect("mid-turn compaction result")
        .expect("mid-turn compaction applied");
        assert_eq!(mid_turn.continuation, CompactionContinuation::MidTurn);
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
                memory_floor_tokens: 0,
                safety_buffer_tokens: 0,
                continuation: CompactionContinuation::MidTurn,
            },
        )
        .await
        .expect_err("cancelled compaction should error");

        assert!(err.to_string().contains("interrupted"));
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
        let outcome = execute_regular_turn(
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
        assert!(delivered.iter().any(
            |event| matches!(event, EventMsg::TurnCompleted { turn_id } if turn_id == "turn-1")
        ));
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
        let first = execute_regular_turn(
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
        let second = execute_regular_turn(
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
            let mut settings = host.regular_turn_settings();
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
        let tool_specs = Vec::<ToolSpec>::new();

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
            &tool_specs,
            &settings,
            BudgetedFragmentInputs {
                raw_memory_fragment: host.raw_memory_fragment(),
                skill_summary: None,
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
            &tool_specs,
            &settings,
            BudgetedFragmentInputs {
                raw_memory_fragment: host.raw_memory_fragment(),
                skill_summary: None,
            },
        );
        assert!(after.fragments.iter().any(|item| {
            matches!(item, crate::ResponseItem::User { content } if crate::input_items_to_plain_text(content).contains("<long_term_memory>"))
        }));
    }
}
