use super::{RuntimeTask, TaskContext, TaskKind};
use crate::tools::ToolBatchRunner;
use crate::{AgentRuntime, emit_event};
use agent_core::{ContextManager, ConversationHistory, ModelUsage, RolloutItem};
use agent_protocol::{
    EventMsg, ServerRequest, ServerRequestDecision, TranscriptItem, TurnItemDeltaKind,
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
    let tool_specs = runtime.tools.specs();
    let mut denied_requests = HashSet::new();
    let environment_context = runtime.environment_context();
    let mut turn_total_usage = ModelUsage::default();

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

        let model_request = context_manager.build_current_model_request_with_fragments(
            std::slice::from_ref(&environment_context),
            tool_specs.clone(),
            runtime.config.llm.temperature,
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
            emit_event(
                &mut events,
                on_event,
                EventMsg::TokenUsageUpdated {
                    turn_id: turn_id.to_string(),
                    last_usage: usage,
                    total_usage: turn_total_usage.clone(),
                    model_context_window: None,
                },
            );
        }

        let assistant_response_item =
            context_manager.record_assistant_message(response.content.clone(), tool_calls.clone());
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
