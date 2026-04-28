use super::{RuntimeTask, TaskContext, TaskKind};
use crate::{AgentRuntime, emit_event, summarize_arguments};
use agent_core::{AgentSession, ModelRequest};
use agent_protocol::{ApprovalDecision, ApprovalRequest, ToolResult, TurnEvent, TurnItemKind, TurnState};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub(crate) struct TurnOutcome {
    pub(crate) turn_id: String,
    pub(crate) final_response: String,
    pub(crate) events: Vec<TurnEvent>,
    pub(crate) session: AgentSession,
    pub(crate) model_name: Option<String>,
    pub(crate) state: TurnState,
}

pub(crate) struct RegularTurnTask;

impl<E, F, Fut> RuntimeTask<E, F, Fut> for RegularTurnTask
where
    E: FnMut(&TurnEvent) + Send,
    F: Fn(ApprovalRequest) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<ApprovalDecision>> + Send,
{
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }

    async fn run(
        self,
        ctx: TaskContext<'_, E>,
        session: AgentSession,
        approval: F,
    ) -> Result<TurnOutcome> {
        execute_regular_turn(
            ctx.runtime,
            ctx.session_id,
            ctx.turn_id,
            ctx.cancellation_token,
            session,
            ctx.on_event,
            approval,
        )
        .await
    }
}

pub(crate) async fn execute_regular_turn<E, F, Fut>(
    runtime: &AgentRuntime,
    session_id: &str,
    turn_id: &str,
    cancellation_token: CancellationToken,
    session: AgentSession,
    on_event: &mut E,
    approval: F,
) -> Result<TurnOutcome>
where
    E: FnMut(&TurnEvent) + Send,
    F: Fn(ApprovalRequest) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<ApprovalDecision>> + Send,
{
    let mut session = session;
    let mut events = Vec::new();
    let mut last_model_name = None;
    let tool_specs = runtime.tools.specs();

    for _ in 0..runtime.policy.max_tool_roundtrips {
        if cancellation_token.is_cancelled() || runtime.is_turn_cancelled(session_id).await {
            emit_event(
                &mut events,
                on_event,
                TurnEvent::TurnCancelled {
                    turn_id: turn_id.to_string(),
                    reason: "interrupted by client".to_string(),
                },
            );
            return Ok(TurnOutcome {
                turn_id: turn_id.to_string(),
                final_response: "Turn cancelled.".to_string(),
                events,
                session,
                model_name: last_model_name,
                state: TurnState::Cancelled,
            });
        }

        emit_event(
            &mut events,
            on_event,
            TurnEvent::ModelRequestStarted {
                turn_id: turn_id.to_string(),
                message_count: session.messages.len(),
                tool_count: tool_specs.len(),
            },
        );

        let response = runtime
            .complete_model_request(
                &cancellation_token,
                ModelRequest {
                    messages: session.messages.clone(),
                    tools: tool_specs.clone(),
                    temperature: runtime.config.llm.temperature,
                },
            )
            .await?;

        last_model_name = response.model_name.clone();
        let tool_calls = response.tool_calls.clone();
        emit_event(
            &mut events,
            on_event,
            TurnEvent::ModelResponseReceived {
                turn_id: turn_id.to_string(),
                model_name: response.model_name.clone(),
                has_content: response.content.is_some(),
                tool_call_count: tool_calls.len(),
            },
        );

        if let Some(content) = response.content.clone() {
            let assistant_item_id = format!("assistant:{turn_id}");
            emit_event(
                &mut events,
                on_event,
                TurnEvent::ItemStarted {
                    turn_id: turn_id.to_string(),
                    item_id: assistant_item_id.clone(),
                    kind: TurnItemKind::AssistantMessage,
                    title: Some("assistant_message".to_string()),
                },
            );
            for delta in assistant_deltas(&content, 96) {
                emit_event(
                    &mut events,
                    on_event,
                    TurnEvent::ItemDelta {
                        turn_id: turn_id.to_string(),
                        item_id: assistant_item_id.clone(),
                        delta: delta.clone(),
                    },
                );
            }
            emit_event(
                &mut events,
                on_event,
                TurnEvent::ItemCompleted {
                    turn_id: turn_id.to_string(),
                    item_id: assistant_item_id,
                },
            );
        }

        session.push_assistant_message(response.content.clone(), tool_calls.clone());

        if tool_calls.is_empty() {
            let final_response = response
                .content
                .unwrap_or_else(|| "The model returned an empty response.".to_string());
            emit_event(
                &mut events,
                on_event,
                TurnEvent::TurnCompleted {
                    turn_id: turn_id.to_string(),
                    final_response: final_response.clone(),
                },
            );
            return Ok(TurnOutcome {
                turn_id: turn_id.to_string(),
                final_response,
                events,
                session,
                model_name: last_model_name,
                state: TurnState::Completed,
            });
        }

        let tool_ctx = runtime
            .context
            .tool_context(session_id.to_string(), cancellation_token.clone());
        for call in tool_calls {
            if cancellation_token.is_cancelled() || runtime.is_turn_cancelled(session_id).await {
                emit_event(
                    &mut events,
                    on_event,
                    TurnEvent::TurnCancelled {
                        turn_id: turn_id.to_string(),
                        reason: "interrupted by client".to_string(),
                    },
                );
                return Ok(TurnOutcome {
                    turn_id: turn_id.to_string(),
                    final_response: "Turn cancelled.".to_string(),
                    events,
                    session,
                    model_name: last_model_name,
                    state: TurnState::Cancelled,
                });
            }

            let tool_item_id = format!("tool:{}", call.id);
            emit_event(
                &mut events,
                on_event,
                TurnEvent::ItemStarted {
                    turn_id: turn_id.to_string(),
                    item_id: tool_item_id.clone(),
                    kind: TurnItemKind::ToolCall,
                    title: Some(call.name.clone()),
                },
            );

            if let Some(spec) = tool_specs.iter().find(|spec| spec.name == call.name)
                && spec.requires_approval
            {
                runtime
                    .state
                    .update_turn_state(session_id, turn_id, TurnState::WaitingForApproval)
                    .await;
                let request = ApprovalRequest {
                    turn_id: turn_id.to_string(),
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    reason: format!("Tool `{}` can modify files or execute commands.", call.name),
                    arguments_preview: summarize_arguments(&call.arguments),
                };
                emit_event(
                    &mut events,
                    on_event,
                    TurnEvent::ApprovalRequested {
                        turn_id: turn_id.to_string(),
                        request: request.clone(),
                    },
                );
                let decision = runtime
                    .await_approval(&cancellation_token, approval(request))
                    .await?;
                runtime
                    .state
                    .update_turn_state(session_id, turn_id, TurnState::Running)
                    .await;
                emit_event(
                    &mut events,
                    on_event,
                    TurnEvent::ApprovalResolved {
                        turn_id: turn_id.to_string(),
                        tool_call_id: call.id.clone(),
                        approved: decision.approved,
                        reason: decision.reason.clone(),
                    },
                );
                if !decision.approved {
                    let reason = decision
                        .reason
                        .unwrap_or_else(|| "approval denied".to_string());
                    let result = ToolResult {
                        tool_call_id: call.id.clone(),
                        name: call.name.clone(),
                        content: format!("Tool execution skipped: {reason}"),
                        summary: "tool execution skipped".to_string(),
                        is_error: true,
                    };
                    emit_event(
                        &mut events,
                        on_event,
                        TurnEvent::ItemDelta {
                            turn_id: turn_id.to_string(),
                            item_id: tool_item_id.clone(),
                            delta: format!("Tool execution skipped: {reason}"),
                        },
                    );
                    emit_event(
                        &mut events,
                        on_event,
                        TurnEvent::ItemCompleted {
                            turn_id: turn_id.to_string(),
                            item_id: tool_item_id.clone(),
                        },
                    );
                    session.push_tool_result(result);
                    continue;
                }
            }

            let result = runtime
                .execute_tool_call(&cancellation_token, call.clone(), &tool_ctx)
                .await?;
            if cancellation_token.is_cancelled() || runtime.is_turn_cancelled(session_id).await {
                emit_event(
                    &mut events,
                    on_event,
                    TurnEvent::TurnCancelled {
                        turn_id: turn_id.to_string(),
                        reason: "interrupted by client".to_string(),
                    },
                );
                return Ok(TurnOutcome {
                    turn_id: turn_id.to_string(),
                    final_response: "Turn cancelled.".to_string(),
                    events,
                    session,
                    model_name: last_model_name,
                    state: TurnState::Cancelled,
                });
            }
            if result.is_error {
                emit_event(
                    &mut events,
                    on_event,
                    TurnEvent::ItemDelta {
                        turn_id: turn_id.to_string(),
                        item_id: tool_item_id.clone(),
                        delta: result.content.clone(),
                    },
                );
            } else {
                emit_event(
                    &mut events,
                    on_event,
                    TurnEvent::ItemDelta {
                        turn_id: turn_id.to_string(),
                        item_id: tool_item_id.clone(),
                        delta: result.summary.clone(),
                    },
                );
            }
            emit_event(
                &mut events,
                on_event,
                TurnEvent::ItemCompleted {
                    turn_id: turn_id.to_string(),
                    item_id: tool_item_id,
                },
            );
            session.push_tool_result(result);
        }
    }

    let final_response =
        "Reached the configured tool roundtrip limit before the model produced a final answer."
            .to_string();
    session.push_assistant_message(Some(final_response.clone()), Vec::new());
    emit_event(
        &mut events,
        on_event,
        TurnEvent::TurnCompleted {
            turn_id: turn_id.to_string(),
            final_response: final_response.clone(),
        },
    );
    Ok(TurnOutcome {
        turn_id: turn_id.to_string(),
        final_response,
        events,
        session,
        model_name: last_model_name,
        state: TurnState::Completed,
    })
}

fn assistant_deltas(content: &str, chunk_chars: usize) -> Vec<String> {
    if content.is_empty() || chunk_chars == 0 {
        return Vec::new();
    }

    let chars: Vec<char> = content.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let end = (start + chunk_chars).min(chars.len());
        out.push(chars[start..end].iter().collect());
        start = end;
    }
    out
}
