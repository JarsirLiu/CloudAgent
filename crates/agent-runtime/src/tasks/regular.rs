use super::{RuntimeTask, TaskContext, TaskKind};
use crate::tools::ToolBatchRunner;
use crate::{AgentRuntime, emit_event};
use agent_core::{
    CompactionSummary, ContextCompactionConfig, ContextFragment, ContextManager,
    ContextInputFilterService, ConversationHistory, FilterPolicy, ModelUsage, RolloutItem, apply_history_compaction,
    build_compaction_summary_request, plan_manual_history_compaction,
};
use agent_protocol::{
    EventMsg, ServerRequest, ServerRequestDecision, TranscriptItem, TurnItemDeltaKind,
    TurnItemKind, TurnState,
};
use anyhow::Result;
use std::collections::HashSet;
use std::fs;
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
    let mut turn_total_usage = ModelUsage::default();
    let mut last_sdk_context_tokens: Option<usize> = None;
    let mut history_len_at_last_sdk_usage: Option<usize> = None;

    for _ in 0..runtime.policy.max_tool_roundtrips {
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
            request_overhead_tokens: estimate_request_overhead_tokens(
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

        let trigger_tokens = ((compaction_config.model_context_window as f32)
            * compaction_config.trigger_ratio) as usize;
        let available_history_tokens = trigger_tokens
            .saturating_sub(compaction_config.request_overhead_tokens)
            .max(1);
        let estimated_total_tokens = if let Some(sdk_tokens) = last_sdk_context_tokens {
            let delta_start = history_len_at_last_sdk_usage
                .unwrap_or_else(|| context_manager.history().messages.len())
                .min(context_manager.history().messages.len());
            let local_delta_tokens =
                estimate_history_tokens(&context_manager.history().messages[delta_start..]);
            sdk_tokens.saturating_add(local_delta_tokens)
        } else {
            estimate_history_tokens(&context_manager.history().messages)
        };

        if estimated_total_tokens > available_history_tokens
            && let Some(compaction_plan) = plan_manual_history_compaction(
                &context_manager.history().messages,
                compaction_config,
                /*minimum_history_tokens*/ 1,
            )
        {
            emit_event(
                &mut events,
                on_event,
                EventMsg::ContextCompactionStarted {
                    turn_id: turn_id.to_string(),
                    estimated_tokens: estimated_total_tokens as u64,
                },
            );
            let pre_message_count = context_manager.history().messages.len();
            let pre_context_tokens_estimate =
                estimate_history_tokens(&context_manager.history().messages) as u64;
            let summary_request = build_compaction_summary_request(
                &compaction_plan,
                compaction_config,
                runtime.config.llm.temperature,
            );
            let summary_response = runtime
                .complete_model_request(&cancellation_token, summary_request)
                .await?;
            let summary = summary_response
                .content
                .map(|text| CompactionSummary::from_model_output(&text).ensure_defaults())
                .filter(|summary| !summary.current_task.is_empty())
                .unwrap_or_else(|| CompactionSummary::fallback_from_plan(&compaction_plan));
            let compacted = apply_history_compaction(
                &mut context_manager.history_mut().messages,
                &compaction_plan,
                summary,
            );
            let post_message_count = compacted.replacement_history.len();
            let post_context_tokens_estimate =
                estimate_history_tokens(&compacted.replacement_history) as u64;
            let rendered_summary = compacted.summary.rendered();
            runtime
                .persist_rollout_items(
                    conversation_id,
                    &[RolloutItem::Compacted {
                        summary: compacted.summary.clone(),
                        rendered_summary: rendered_summary.clone(),
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
                    pre_context_tokens_estimate,
                    post_context_tokens_estimate,
                    pre_message_count,
                    post_message_count,
                    preserved_tail_count: post_message_count.saturating_sub(2),
                },
            );
        }

        let mut model_request = context_manager.build_current_model_request_with_fragments(
            std::slice::from_ref(&environment_context),
            tool_specs.clone(),
            runtime.config.llm.temperature,
        );
        let filter_service = ContextInputFilterService::new();
        model_request.messages = filter_service.filter_for_model(
            model_request.messages,
            FilterPolicy {
                enabled: is_global_pre_llm_filter_enabled(runtime),
            },
        );

        emit_event(
            &mut events,
            on_event,
            EventMsg::ModelRequestStarted {
                turn_id: turn_id.to_string(),
                message_count: model_request.messages.len(),
                tool_count: tool_specs.len(),
            },
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

fn is_global_pre_llm_filter_enabled(runtime: &AgentRuntime) -> bool {
    let path = runtime
        .config
        .workspace_root
        .join("data")
        .join("ui-settings.json");
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("pre_llm_filter_enabled")
                .and_then(|b| b.as_bool())
        })
        .unwrap_or(false)
}

pub(crate) fn estimate_request_overhead_tokens(
    history_messages: &[agent_core::ResponseItem],
    environment_fragment: &agent_core::ResponseItem,
    tool_specs: &[agent_core::ToolSpec],
    minimum_overhead_tokens: usize,
) -> usize {
    let system_tokens = history_messages
        .first()
        .map(|item| estimate_history_tokens(std::slice::from_ref(item)))
        .unwrap_or(0);
    let environment_tokens = estimate_history_tokens(std::slice::from_ref(environment_fragment));
    let tool_tokens = tool_specs
        .iter()
        .map(|tool| {
            tool.name.chars().count()
                + tool.description.chars().count()
                + tool.parameters.to_string().chars().count()
                + 64
        })
        .sum::<usize>()
        .saturating_div(3)
        .max(1);

    minimum_overhead_tokens.max(
        system_tokens
            .saturating_add(environment_tokens)
            .saturating_add(tool_tokens)
            .saturating_add(2_000),
    )
}

pub(crate) fn estimate_history_tokens(messages: &[agent_core::ResponseItem]) -> usize {
    messages
        .iter()
        .map(|item| match item {
            agent_core::ResponseItem::System { content }
            | agent_core::ResponseItem::User { content } => content.chars().count(),
            agent_core::ResponseItem::Assistant {
                content,
                tool_calls,
            } => {
                let text_len = content.as_ref().map_or(0, |text| text.chars().count());
                let tool_len: usize = tool_calls
                    .iter()
                    .map(|call| {
                        call.name.chars().count() + call.arguments.to_string().chars().count()
                    })
                    .sum();
                text_len + tool_len
            }
            agent_core::ResponseItem::Tool { name, content, .. } => {
                name.chars().count() + content.chars().count()
            }
        })
        .sum::<usize>()
        .saturating_div(3)
        .max(1)
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
