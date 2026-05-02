use crate::{AgentRuntime, emit_event, summarize_arguments};
use agent_core::{ContextManager, RolloutItem};
use agent_protocol::{
    CommandExecutionStatus, EventMsg, ServerRequest, ServerRequestDecision, StructuredToolResult,
    ToolApprovalRequest, ToolCall, ToolResult, ToolSpec, TranscriptItem, TurnItemDeltaKind,
    TurnItemKind, TurnState, WriteFileStatus,
};
use anyhow::Result;
use std::collections::HashSet;
use tokio_util::sync::CancellationToken;

pub(crate) struct ToolBatchOutcome {
    pub(crate) cancelled: bool,
}

enum ApprovalFlow {
    Approved,
    Denied,
    Cancelled,
}

pub(crate) struct ToolBatchRunner<'a> {
    runtime: &'a AgentRuntime,
    conversation_id: &'a str,
    turn_id: &'a str,
    cancellation_token: CancellationToken,
    tool_specs: &'a [ToolSpec],
}

impl<'a> ToolBatchRunner<'a> {
    pub(crate) fn new(
        runtime: &'a AgentRuntime,
        conversation_id: &'a str,
        turn_id: &'a str,
        cancellation_token: CancellationToken,
        tool_specs: &'a [ToolSpec],
    ) -> Self {
        Self {
            runtime,
            conversation_id,
            turn_id,
            cancellation_token,
            tool_specs,
        }
    }

    pub(crate) async fn run<E, F, Fut>(
        &self,
        tool_calls: Vec<ToolCall>,
        context_manager: &mut ContextManager,
        events: &mut Vec<EventMsg>,
        on_event: &mut E,
        approval: &F,
        denied_requests: &mut HashSet<String>,
    ) -> Result<ToolBatchOutcome>
    where
        E: FnMut(&EventMsg) + Send,
        F: Fn(ServerRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
    {
        let tool_ctx = self.runtime.context.tool_context(
            self.conversation_id.to_string(),
            self.cancellation_token.clone(),
        );
        let denied_requests_at_batch_start = denied_requests.clone();

        for call in tool_calls {
            if self.is_cancelled().await {
                self.emit_cancelled(events, on_event);
                return Ok(ToolBatchOutcome { cancelled: true });
            }

            let tool_item_id = format!("tool:{}", call.id);
            let Some(spec) = self.spec_for(&call) else {
                self.emit_missing_tool(&call, &tool_item_id, context_manager, events, on_event)
                    .await?;
                continue;
            };

            emit_event(
                events,
                on_event,
                EventMsg::ItemStarted {
                    turn_id: self.turn_id.to_string(),
                    item_id: tool_item_id.clone(),
                    kind: spec.item_kind.clone(),
                    title: Some(tool_item_title(&call)),
                },
            );

            let request_key = tool_request_key(&call);
            if denied_requests_at_batch_start.contains(&request_key) {
                self.record_denied_tool_result(
                    &call,
                    spec,
                    &tool_item_id,
                    context_manager,
                    events,
                    on_event,
                    &repeated_rejection_message(&call.name),
                )
                .await?;
                continue;
            }

            if spec.requires_approval && !self.runtime.is_tool_approved_for_session(&call) {
                let approved = self
                    .request_approval(
                        &call,
                        spec,
                        &tool_item_id,
                        context_manager,
                        events,
                        on_event,
                        approval,
                        denied_requests,
                        request_key,
                    )
                    .await?;
                match approved {
                    ApprovalFlow::Approved => {}
                    ApprovalFlow::Denied => continue,
                    ApprovalFlow::Cancelled => {
                        self.emit_cancelled(events, on_event);
                        return Ok(ToolBatchOutcome { cancelled: true });
                    }
                }
            }

            let mut tool_streamed_output = false;
            let result = self
                .runtime
                .execute_tool_call_streaming(
                    &self.cancellation_token,
                    call.clone(),
                    &tool_ctx,
                    |delta| {
                        tool_streamed_output = true;
                        let rendered = match delta.stream {
                            agent_protocol::ToolOutputStream::Stdout => delta.chunk,
                            agent_protocol::ToolOutputStream::Stderr => {
                                format!("stderr: {}", delta.chunk)
                            }
                        };
                        emit_event(
                            events,
                            on_event,
                            EventMsg::ItemDelta {
                                turn_id: self.turn_id.to_string(),
                                item_id: tool_item_id.clone(),
                                kind: spec.delta_kind.clone(),
                                delta: rendered,
                            },
                        );
                    },
                )
                .await?;

            if self.is_cancelled().await {
                self.emit_cancelled(events, on_event);
                return Ok(ToolBatchOutcome { cancelled: true });
            }

            if !tool_streamed_output && !result.content.trim().is_empty() {
                emit_event(
                    events,
                    on_event,
                    EventMsg::ItemDelta {
                        turn_id: self.turn_id.to_string(),
                        item_id: tool_item_id.clone(),
                        kind: spec.delta_kind.clone(),
                        delta: result.content.clone(),
                    },
                );
            }
            emit_event(
                events,
                on_event,
                EventMsg::ItemCompleted {
                    turn_id: self.turn_id.to_string(),
                    item_id: tool_item_id.clone(),
                    item: transcript_item_from_tool_result(&tool_item_id, &call.name, &result),
                },
            );
            self.record_tool_result(context_manager, result).await?;
        }

        Ok(ToolBatchOutcome { cancelled: false })
    }

    async fn request_approval<E, F, Fut>(
        &self,
        call: &ToolCall,
        spec: &ToolSpec,
        tool_item_id: &str,
        context_manager: &mut ContextManager,
        events: &mut Vec<EventMsg>,
        on_event: &mut E,
        approval: &F,
        denied_requests: &mut HashSet<String>,
        request_key: String,
    ) -> Result<ApprovalFlow>
    where
        E: FnMut(&EventMsg) + Send,
        F: Fn(ServerRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
    {
        self.runtime
            .state
            .update_turn_state(
                self.conversation_id,
                self.turn_id,
                TurnState::WaitingForServerRequest,
            )
            .await;
        let request = ServerRequest::ToolApproval {
            request: ToolApprovalRequest {
                turn_id: self.turn_id.to_string(),
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                reason: spec
                    .approval_reason
                    .clone()
                    .unwrap_or_else(|| format!("Tool `{}` requires approval.", call.name)),
                arguments_preview: summarize_arguments(&call.arguments),
            },
        };
        emit_event(
            events,
            on_event,
            EventMsg::ServerRequestRequested {
                turn_id: self.turn_id.to_string(),
                request: request.clone(),
            },
        );
        let decision = self
            .runtime
            .await_approval(&self.cancellation_token, approval(request.clone()))
            .await?;
        self.runtime
            .state
            .update_turn_state(self.conversation_id, self.turn_id, TurnState::Running)
            .await;
        emit_event(
            events,
            on_event,
            EventMsg::ServerRequestResolved {
                turn_id: self.turn_id.to_string(),
                request: request.clone(),
                decision: decision.clone(),
            },
        );
        if decision.is_approved() {
            if matches!(
                decision.decision,
                agent_protocol::ServerRequestDecisionKind::AcceptForSession
            ) {
                self.runtime.approve_tool_for_session(call);
            }
            return Ok(ApprovalFlow::Approved);
        }
        if matches!(
            decision.decision,
            agent_protocol::ServerRequestDecisionKind::Cancel
        ) {
            return Ok(ApprovalFlow::Cancelled);
        }

        let reason = decision
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| default_rejection_message(&call.name));
        denied_requests.insert(request_key);
        self.record_denied_tool_result(
            call,
            spec,
            tool_item_id,
            context_manager,
            events,
            on_event,
            &reason,
        )
        .await?;
        Ok(ApprovalFlow::Denied)
    }

    async fn record_denied_tool_result<E>(
        &self,
        call: &ToolCall,
        spec: &ToolSpec,
        tool_item_id: &str,
        context_manager: &mut ContextManager,
        events: &mut Vec<EventMsg>,
        on_event: &mut E,
        reason: &str,
    ) -> Result<()>
    where
        E: FnMut(&EventMsg) + Send,
    {
        let content = reason.to_string();
        let result = ToolResult {
            tool_call_id: call.id.clone(),
            name: call.name.clone(),
            content: content.clone(),
            is_error: true,
            structured: denied_tool_result(call.name.as_str(), &call.arguments, reason.to_string()),
        };
        emit_event(
            events,
            on_event,
            EventMsg::ItemDelta {
                turn_id: self.turn_id.to_string(),
                item_id: tool_item_id.to_string(),
                kind: spec.delta_kind.clone(),
                delta: content,
            },
        );
        emit_event(
            events,
            on_event,
            EventMsg::ItemCompleted {
                turn_id: self.turn_id.to_string(),
                item_id: tool_item_id.to_string(),
                item: denied_transcript_item(tool_item_id, &call.name, &call.arguments, reason),
            },
        );
        self.record_tool_result(context_manager, result).await
    }

    async fn record_tool_result(
        &self,
        context_manager: &mut ContextManager,
        result: ToolResult,
    ) -> Result<()> {
        let tool_response_item = context_manager.record_tool_result(result);
        self.runtime
            .persist_rollout_items(
                self.conversation_id,
                &[RolloutItem::from(tool_response_item)],
            )
            .await?;
        self.runtime
            .state
            .save_history(context_manager.history().clone())
            .await;
        Ok(())
    }

    async fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
            || self.runtime.is_turn_cancelled(self.conversation_id).await
    }

    fn emit_cancelled(&self, events: &mut Vec<EventMsg>, on_event: &mut impl FnMut(&EventMsg)) {
        emit_event(
            events,
            on_event,
            EventMsg::TurnCancelled {
                turn_id: self.turn_id.to_string(),
                reason: "interrupted by client".to_string(),
            },
        );
    }

    async fn emit_missing_tool<E>(
        &self,
        call: &ToolCall,
        tool_item_id: &str,
        context_manager: &mut ContextManager,
        events: &mut Vec<EventMsg>,
        on_event: &mut E,
    ) -> Result<()>
    where
        E: FnMut(&EventMsg) + Send,
    {
        let message = format!("Tool `{}` is not registered.", call.name);
        emit_event(
            events,
            on_event,
            EventMsg::ItemStarted {
                turn_id: self.turn_id.to_string(),
                item_id: tool_item_id.to_string(),
                kind: TurnItemKind::ToolCall,
                title: Some(call.name.clone()),
            },
        );
        emit_event(
            events,
            on_event,
            EventMsg::ItemDelta {
                turn_id: self.turn_id.to_string(),
                item_id: tool_item_id.to_string(),
                kind: TurnItemDeltaKind::ToolOutput,
                delta: message.clone(),
            },
        );
        let result = ToolResult {
            tool_call_id: call.id.clone(),
            name: call.name.clone(),
            content: message,
            is_error: true,
            structured: None,
        };
        emit_event(
            events,
            on_event,
            EventMsg::ItemCompleted {
                turn_id: self.turn_id.to_string(),
                item_id: tool_item_id.to_string(),
                item: transcript_item_from_tool_result(tool_item_id, &call.name, &result),
            },
        );
        self.record_tool_result(context_manager, result).await
    }

    fn spec_for(&self, call: &ToolCall) -> Option<&ToolSpec> {
        self.tool_specs.iter().find(|spec| spec.name == call.name)
    }
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
            aggregated_output,
            duration_ms,
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
            aggregated_output: aggregated_output.clone(),
            duration_ms: *duration_ms,
            summary: result.content.clone(),
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
            summary: result.content.clone(),
        },
        _ => TranscriptItem::ToolResult {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            content: result.content.clone(),
            summary: result.content.clone(),
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
            aggregated_output,
            duration_ms,
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
            aggregated_output,
            duration_ms,
            summary: reason.to_string(),
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
            summary: reason.to_string(),
        },
        structured => TranscriptItem::ToolResult {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            content: reason.to_string(),
            summary: reason.to_string(),
            structured,
        },
    }
}

fn tool_item_title(call: &ToolCall) -> String {
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

fn tool_request_key(call: &ToolCall) -> String {
    format!("{}:{}", call.name, canonical_json(&call.arguments))
}

fn canonical_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
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
                aggregated_output: None,
                duration_ms: None,
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
        "edit_file" | "apply_patch" => Some(StructuredToolResult::ApplyPatch {
            files_changed: 0,
            status: WriteFileStatus::Declined,
        }),
        _ => None,
    }
}

fn default_rejection_message(tool_name: &str) -> String {
    match tool_name {
        "shell_command" => {
            "exec command rejected by user: the user denied this approval request; do not describe this as a system safety restriction".to_string()
        }
        "write_file" => {
            "patch rejected by user: the user denied this approval request; do not describe this as a system safety restriction".to_string()
        }
        "edit_file" | "apply_patch" => {
            "edit rejected by user: the user denied this approval request; do not describe this as a system safety restriction".to_string()
        }
        _ => {
            "tool call rejected by user: the user denied this approval request; do not describe this as a system safety restriction".to_string()
        }
    }
}

fn repeated_rejection_message(tool_name: &str) -> String {
    format!(
        "{}; same tool request was already denied in this turn",
        default_rejection_message(tool_name)
    )
}
