use super::{RuntimeTask, TaskContext, TaskKind};
use crate::{AgentRuntime, emit_event, summarize_arguments};
use agent_core::{ContextManager, ConversationHistory, RolloutItem};
use agent_protocol::{
    CommandExecutionStatus, EventMsg, ServerRequest, ServerRequestDecision, StructuredToolResult,
    ToolApprovalRequest, ToolResult, TranscriptItem, TurnItemDeltaKind, TurnItemKind, TurnState,
    WriteFileStatus,
};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub(crate) struct TurnOutcome {
    pub(crate) turn_id: String,
    pub(crate) final_response: String,
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
                final_response: "Turn cancelled.".to_string(),
                events,
                history: context_manager.history().clone(),
                model_name: last_model_name,
                state: TurnState::Cancelled,
            });
        }

        emit_event(
            &mut events,
            on_event,
            EventMsg::ModelRequestStarted {
                turn_id: turn_id.to_string(),
                message_count: context_manager.history().messages.len(),
                tool_count: tool_specs.len(),
            },
        );

        let mut streaming_assistant_item_id: Option<String> = None;
        let response = runtime
            .complete_model_request_streaming(
                &cancellation_token,
                context_manager.build_current_model_request(
                    tool_specs.clone(),
                    runtime.config.llm.temperature,
                ),
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
            let final_response = response
                .content
                .clone()
                .unwrap_or_else(|| "The model returned an empty response.".to_string());
            if !had_streaming_assistant_item && response.content.is_none() {
                emit_assistant_item(
                    &mut events,
                    on_event,
                    turn_id,
                    &final_response,
                    &mut assistant_item_seq,
                );
            }
            emit_event(
                &mut events,
                on_event,
                EventMsg::TurnCompleted {
                    turn_id: turn_id.to_string(),
                    final_response: final_response.clone(),
                },
            );
            return Ok(TurnOutcome {
                turn_id: turn_id.to_string(),
                final_response,
                events,
                history: context_manager.history().clone(),
                model_name: last_model_name,
                state: TurnState::Completed,
            });
        }

        let tool_ctx = runtime
            .context
            .tool_context(conversation_id.to_string(), cancellation_token.clone());
        for call in tool_calls {
            if cancellation_token.is_cancelled() || runtime.is_turn_cancelled(conversation_id).await
            {
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
                    final_response: "Turn cancelled.".to_string(),
                    events,
                    history: context_manager.history().clone(),
                    model_name: last_model_name,
                    state: TurnState::Cancelled,
                });
            }

            let tool_item_id = format!("tool:{}", call.id);
            let tool_item_kind = tool_item_kind(&call.name);
            emit_event(
                &mut events,
                on_event,
                EventMsg::ItemStarted {
                    turn_id: turn_id.to_string(),
                    item_id: tool_item_id.clone(),
                    kind: tool_item_kind,
                    title: Some(tool_item_title(&call)),
                },
            );

            if let Some(spec) = tool_specs.iter().find(|spec| spec.name == call.name)
                && spec.requires_approval
            {
                runtime
                    .state
                    .update_turn_state(conversation_id, turn_id, TurnState::WaitingForServerRequest)
                    .await;
                let request = ServerRequest::ToolApproval {
                    request: ToolApprovalRequest {
                        turn_id: turn_id.to_string(),
                        tool_call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        reason: format!(
                            "Tool `{}` can modify files or execute commands.",
                            call.name
                        ),
                        arguments_preview: summarize_arguments(&call.arguments),
                    },
                };
                emit_event(
                    &mut events,
                    on_event,
                    EventMsg::ServerRequestRequested {
                        turn_id: turn_id.to_string(),
                        request: request.clone(),
                    },
                );
                let decision = runtime
                    .await_approval(&cancellation_token, approval(request.clone()))
                    .await?;
                runtime
                    .state
                    .update_turn_state(conversation_id, turn_id, TurnState::Running)
                    .await;
                emit_event(
                    &mut events,
                    on_event,
                    EventMsg::ServerRequestResolved {
                        turn_id: turn_id.to_string(),
                        request: request.clone(),
                        decision: decision.clone(),
                    },
                );
                if !decision.approved {
                    let reason = decision
                        .reason
                        .unwrap_or_else(|| "request denied".to_string());
                    let result = ToolResult {
                        tool_call_id: call.id.clone(),
                        name: call.name.clone(),
                        content: format!("Tool execution skipped: {reason}"),
                        summary: "tool execution skipped".to_string(),
                        is_error: true,
                        structured: denied_tool_result(
                            call.name.as_str(),
                            &call.arguments,
                            reason.clone(),
                        ),
                    };
                    emit_event(
                        &mut events,
                        on_event,
                        EventMsg::ItemDelta {
                            turn_id: turn_id.to_string(),
                            item_id: tool_item_id.clone(),
                            kind: tool_delta_kind(&call.name),
                            delta: format!("Tool execution skipped: {reason}"),
                        },
                    );
                    emit_event(
                        &mut events,
                        on_event,
                        EventMsg::ItemCompleted {
                            turn_id: turn_id.to_string(),
                            item_id: tool_item_id.clone(),
                            item: denied_transcript_item(
                                &tool_item_id,
                                &call.name,
                                &call.arguments,
                                &reason,
                            ),
                        },
                    );
                    let tool_response_item = context_manager.record_tool_result(result);
                    runtime
                        .persist_rollout_items(
                            conversation_id,
                            &[RolloutItem::from(tool_response_item)],
                        )
                        .await?;
                    runtime
                        .state
                        .save_history(context_manager.history().clone())
                        .await;
                    // Stop processing the remaining tool calls from the same assistant output.
                    // Let the model consume this denial result first in the next roundtrip.
                    break;
                }
            }

            let result = runtime
                .execute_tool_call(&cancellation_token, call.clone(), &tool_ctx)
                .await?;
            if cancellation_token.is_cancelled() || runtime.is_turn_cancelled(conversation_id).await
            {
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
                    final_response: "Turn cancelled.".to_string(),
                    events,
                    history: context_manager.history().clone(),
                    model_name: last_model_name,
                    state: TurnState::Cancelled,
                });
            }
            emit_event(
                &mut events,
                on_event,
                EventMsg::ItemDelta {
                    turn_id: turn_id.to_string(),
                    item_id: tool_item_id.clone(),
                    kind: tool_delta_kind(&call.name),
                    delta: result.summary.clone(),
                },
            );
            emit_event(
                &mut events,
                on_event,
                EventMsg::ItemCompleted {
                    turn_id: turn_id.to_string(),
                    item_id: tool_item_id.clone(),
                    item: transcript_item_from_tool_result(&tool_item_id, &call.name, &result),
                },
            );
            let tool_response_item = context_manager.record_tool_result(result);
            runtime
                .persist_rollout_items(conversation_id, &[RolloutItem::from(tool_response_item)])
                .await?;
            runtime
                .state
                .save_history(context_manager.history().clone())
                .await;
        }
    }

    let final_response =
        "Reached the configured tool roundtrip limit before the model produced a final answer."
            .to_string();
    emit_assistant_item(
        &mut events,
        on_event,
        turn_id,
        &final_response,
        &mut assistant_item_seq,
    );
    let final_response_item =
        context_manager.record_assistant_message(Some(final_response.clone()), Vec::new());
    runtime
        .persist_rollout_items(conversation_id, &[RolloutItem::from(final_response_item)])
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
            final_response: final_response.clone(),
        },
    );
    Ok(TurnOutcome {
        turn_id: turn_id.to_string(),
        final_response,
        events,
        history: context_manager.history().clone(),
        model_name: last_model_name,
        state: TurnState::Completed,
    })
}

fn transcript_item_from_tool_result(
    item_id: &str,
    tool_name: &str,
    result: &ToolResult,
) -> TranscriptItem {
    match &result.structured {
        Some(StructuredToolResult::CommandExecution {
            command,
            current_directory,
            status,
            exit_code,
            stdout,
            stderr,
            ..
        }) => TranscriptItem::CommandExecution {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            command: command.clone(),
            current_directory: current_directory.clone(),
            status: status.clone(),
            exit_code: *exit_code,
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            summary: result.summary.clone(),
        },
        Some(StructuredToolResult::WriteFile {
            path,
            bytes_written,
            status,
        }) => TranscriptItem::FileChange {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            path: path.clone(),
            status: status.clone(),
            bytes_written: *bytes_written,
            summary: result.summary.clone(),
        },
        _ => TranscriptItem::ToolResult {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            content: result.content.clone(),
            summary: result.summary.clone(),
            structured: result.structured.clone(),
        },
    }
}

fn denied_transcript_item(
    item_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
    reason: &str,
) -> TranscriptItem {
    match denied_tool_result(tool_name, arguments, reason.to_string()) {
        Some(StructuredToolResult::CommandExecution {
            command,
            current_directory,
            status,
            exit_code,
            stdout,
            stderr,
            ..
        }) => TranscriptItem::CommandExecution {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            command,
            current_directory,
            status,
            exit_code,
            stdout,
            stderr,
            summary: "tool execution skipped".to_string(),
        },
        Some(StructuredToolResult::WriteFile {
            path,
            bytes_written,
            status,
        }) => TranscriptItem::FileChange {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            path,
            status,
            bytes_written,
            summary: "tool execution skipped".to_string(),
        },
        structured => TranscriptItem::ToolResult {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            content: "tool execution skipped".to_string(),
            summary: "tool execution skipped".to_string(),
            structured,
        },
    }
}

fn tool_item_kind(tool_name: &str) -> TurnItemKind {
    match tool_name {
        "shell_command" => TurnItemKind::CommandExecution,
        "write_file" => TurnItemKind::FileChange,
        _ => TurnItemKind::ToolCall,
    }
}

fn tool_delta_kind(tool_name: &str) -> TurnItemDeltaKind {
    match tool_name {
        "shell_command" => TurnItemDeltaKind::CommandExecutionOutput,
        "write_file" => TurnItemDeltaKind::FileChangeOutput,
        _ => TurnItemDeltaKind::ToolOutput,
    }
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

fn tool_item_title(call: &agent_core::ToolCall) -> String {
    if call.name == "shell_command"
        && let Some(command) = call
            .arguments
            .get("command")
            .and_then(|value| value.as_str())
        && !command.trim().is_empty()
    {
        return command.trim().to_string();
    }
    call.name.clone()
}

fn denied_tool_result(
    tool_name: &str,
    arguments: &serde_json::Value,
    reason: String,
) -> Option<StructuredToolResult> {
    match tool_name {
        "shell_command" => {
            let command = arguments
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let current_directory = arguments
                .get("workdir")
                .and_then(|value| value.as_str())
                .unwrap_or(".")
                .to_string();
            Some(StructuredToolResult::CommandExecution {
                command,
                current_directory,
                status: CommandExecutionStatus::Declined,
                exit_code: None,
                success: Some(false),
                stdout: None,
                stderr: Some(reason),
            })
        }
        "write_file" => {
            let path = arguments
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            Some(StructuredToolResult::WriteFile {
                path,
                bytes_written: 0,
                status: WriteFileStatus::Declined,
            })
        }
        _ => None,
    }
}
