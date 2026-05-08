use super::compaction::{CompactionMode, maybe_compact_history};
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
use std::collections::{BTreeMap, HashSet};
use tokio_util::sync::CancellationToken;

fn apply_signed_delta(base: usize, current: usize, previous: usize) -> usize {
    if current >= previous {
        base.saturating_add(current - previous)
    } else {
        base.saturating_sub(previous - current)
    }
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
        let mut candidate_request = context_manager
            .build_current_model_request_with_rendered_fragments(
                &budgeted.fragments,
                tool_specs.clone(),
                settings.llm_temperature,
            );
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
            CompactionMode::Automatic {
                estimated_total_tokens: estimated_total_tokens
                    .max(compaction_estimated_total_tokens),
                memory_floor_tokens: settings.post_compact_memory_floor_tokens,
                safety_buffer_tokens: settings.context_budget_safety_buffer_tokens,
            },
        )
        .await?;
        if compaction.is_some() {
            emit_event(
                &mut events,
                on_event,
                EventMsg::ContextCompactionStarted {
                    turn_id: turn_id.to_string(),
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
        let model_request = context_facade
            .prepare_model_request(
                &context_manager,
                &settings.workspace_root,
                filter_policy,
                budgeted.fragments.clone(),
                tool_specs.clone(),
                settings.llm_temperature,
            )
            .model_request;
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
                        delta,
                    },
                );
            }

            fn on_reasoning_delta(&mut self, delta: String) {
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
                        kind: TurnItemDeltaKind::ReasoningText,
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

        let assistant_response_item =
            context_manager.record_assistant_message(response.content.clone(), tool_calls.clone());
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
    use super::execute_regular_turn;
    use super::{collect_discoverable_tools, compose_visible_tool_specs};
    use crate::context::EnvironmentContext;
    use crate::tool::RegularTurnToolExposure;
    use crate::turn::{
        RegularTurnSettings, ServerRequest, ServerRequestDecision, ServerRequestHandler,
        ToolBatchOutcome, TurnHost,
    };
    use crate::{
        ContextManager, ConversationHistory, EventMsg, ModelRequest, ModelResponse,
        ModelStreamObserver, ModelUsage, RolloutItem, ToolCall, ToolExecutionPolicy, ToolIdentity,
        ToolSource, ToolSpec, TurnItemDeltaKind, TurnItemKind, TurnOutcome,
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
            let root = std::env::temp_dir().join(format!(
                "cloudagent-agent-core-test-{}",
                Uuid::now_v7()
            ));
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

    struct MockTurnHost {
        responses: Mutex<Vec<ModelResponse>>,
        workspace: Arc<TestWorkspace>,
    }

    impl MockTurnHost {
        fn new(responses: Vec<ModelResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                workspace: Arc::new(TestWorkspace::new()),
            }
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
            RegularTurnSettings {
                workspace_root: self.workspace.path().to_path_buf(),
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
                enable_skill_bucket: false,
                enable_mcp_bucket: false,
            }
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
            None
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
            _cancellation_token: &CancellationToken,
            _request: ModelRequest,
        ) -> Result<ModelResponse> {
            unreachable!()
        }

        async fn complete_model_request_streaming(
            &self,
            _cancellation_token: &CancellationToken,
            _request: ModelRequest,
            observer: &mut dyn ModelStreamObserver,
        ) -> Result<ModelResponse> {
            let response = self.responses.lock().expect("responses lock").remove(0);
            if let Some(reasoning) = response.reasoning.clone() {
                observer.on_reasoning_delta(reasoning);
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

        fn audit_turn_started(&self, _conversation_id: &str, _user_input: &str) {}
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
                model_name: Some("test-model".to_string()),
                usage: None,
            },
            ModelResponse {
                content: Some("final answer".to_string()),
                reasoning: Some("second reasoning".to_string()),
                tool_calls: vec![],
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
}
