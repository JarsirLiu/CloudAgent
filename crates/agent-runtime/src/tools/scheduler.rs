use crate::tools::parallel::{ParallelToolInvocation, run_parallel_tool_invocations};
use crate::{AgentRuntime, emit_event, summarize_arguments};
use agent_tools::registry::ToolBatchExecutionStrategy;
use agent_core::{ContextManager, RolloutItem};
use agent_protocol::{
    ApprovalPolicy, EventMsg, PermissionProfile, ServerRequest, ServerRequestDecision,
    ToolApprovalRequest, ToolCall, ToolResult, ToolSpec, TurnItemDeltaKind, TurnItemKind,
    TurnState,
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

struct ReadyToolCall<'a> {
    call: ToolCall,
    spec: &'a ToolSpec,
    tool_item_id: String,
}

pub(crate) struct ToolBatchRunner<'a> {
    runtime: &'a AgentRuntime,
    conversation_id: &'a str,
    turn_id: &'a str,
    permission_profile: &'a PermissionProfile,
    approval_policy: &'a ApprovalPolicy,
    cancellation_token: CancellationToken,
    tool_specs: &'a [ToolSpec],
}

impl<'a> ToolBatchRunner<'a> {
    pub(crate) fn new(
        runtime: &'a AgentRuntime,
        conversation_id: &'a str,
        turn_id: &'a str,
        permission_profile: &'a PermissionProfile,
        approval_policy: &'a ApprovalPolicy,
        cancellation_token: CancellationToken,
        tool_specs: &'a [ToolSpec],
    ) -> Self {
        Self {
            runtime,
            conversation_id,
            turn_id,
            permission_profile,
            approval_policy,
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
        let mut ready_calls = Vec::new();

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
                    title: Some(self.runtime.tools.tool_item_title(&call)),
                },
            );
            self.runtime.audit().tool_started(
                self.conversation_id,
                self.turn_id,
                &call,
                summarize_arguments(&call.arguments),
            );

            let request_key = self.runtime.tools.tool_request_key(&call);
            if denied_requests_at_batch_start.contains(&request_key) {
                self.record_denied_tool_result(
                    &call,
                    spec,
                    &tool_item_id,
                    context_manager,
                    events,
                    on_event,
                    &self.runtime.tools.repeated_rejection_message(&call.name),
                )
                .await?;
                continue;
            }

            let approval_requirement =
                self.runtime.tools.approval_requirement_for_call(
                    spec,
                    &call,
                    &self.runtime.context.workspace_root,
                    self.permission_profile,
                    self.approval_policy,
                );
            if approval_requirement.requires_approval
                && !self.runtime.is_tool_approved_for_session(&call)
            {
                let approved = self
                    .request_approval(
                        &call,
                        spec,
                        approval_requirement.reason.as_deref(),
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
            ready_calls.push(ReadyToolCall {
                call,
                spec,
                tool_item_id,
            });
        }

        match self.batch_execution_strategy(&ready_calls) {
            ToolBatchExecutionStrategy::Parallel => {
            self.run_parallel_ready_calls(
                ready_calls,
                &tool_ctx,
                context_manager,
                events,
                on_event,
            )
            .await?
            }
            ToolBatchExecutionStrategy::Sequential => {
                self.run_ready_calls_sequentially(
                ready_calls,
                &tool_ctx,
                context_manager,
                events,
                on_event,
            )
            .await?
            }
        }

        Ok(ToolBatchOutcome { cancelled: false })
    }

    async fn run_ready_calls_sequentially<E>(
        &self,
        ready_calls: Vec<ReadyToolCall<'_>>,
        tool_ctx: &agent_core::ToolExecutionContext,
        context_manager: &mut ContextManager,
        events: &mut Vec<EventMsg>,
        on_event: &mut E,
    ) -> Result<()>
    where
        E: FnMut(&EventMsg) + Send,
    {
        for ready in ready_calls {
            let mut tool_streamed_output = false;
            let result = self
                .runtime
                .execute_tool_call_streaming(
                    &self.cancellation_token,
                    ready.call.clone(),
                    tool_ctx,
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
                                item_id: ready.tool_item_id.clone(),
                                kind: ready.spec.delta_kind.clone(),
                                delta: rendered,
                            },
                        );
                    },
                )
                .await?;

            if self.is_cancelled().await {
                self.emit_cancelled(events, on_event);
                anyhow::bail!(crate::TURN_INTERRUPTED_ERROR);
            }

            self.emit_finished_tool(
                &ready.call,
                ready.spec.delta_kind.clone(),
                &ready.tool_item_id,
                tool_streamed_output,
                result,
                context_manager,
                events,
                on_event,
            )
            .await?;
        }
        Ok(())
    }

    async fn run_parallel_ready_calls<E>(
        &self,
        ready_calls: Vec<ReadyToolCall<'_>>,
        tool_ctx: &agent_core::ToolExecutionContext,
        context_manager: &mut ContextManager,
        events: &mut Vec<EventMsg>,
        on_event: &mut E,
    ) -> Result<()>
    where
        E: FnMut(&EventMsg) + Send,
    {
        let invocations = ready_calls
            .into_iter()
            .enumerate()
            .map(|(index, ready)| ParallelToolInvocation {
                index,
                call: ready.call,
                tool_item_id: ready.tool_item_id,
                delta_kind: ready.spec.delta_kind.clone(),
            })
            .collect::<Vec<_>>();
        let results = run_parallel_tool_invocations(
            self.runtime,
            tool_ctx,
            &self.cancellation_token,
            invocations,
        )
        .await?;

        for finished in results {
            self.emit_finished_tool(
                &finished.call,
                finished.delta_kind,
                &finished.tool_item_id,
                false,
                finished.result,
                context_manager,
                events,
                on_event,
            )
            .await?;
        }
        Ok(())
    }

    async fn emit_finished_tool<E>(
        &self,
        call: &ToolCall,
        delta_kind: TurnItemDeltaKind,
        tool_item_id: &str,
        tool_streamed_output: bool,
        result: ToolResult,
        context_manager: &mut ContextManager,
        events: &mut Vec<EventMsg>,
        on_event: &mut E,
    ) -> Result<()>
    where
        E: FnMut(&EventMsg) + Send,
    {
        if !tool_streamed_output && !result.content.trim().is_empty() {
            emit_event(
                events,
                on_event,
                EventMsg::ItemDelta {
                    turn_id: self.turn_id.to_string(),
                    item_id: tool_item_id.to_string(),
                    kind: delta_kind,
                    delta: result.content.clone(),
                },
            );
        }
        emit_event(
            events,
            on_event,
            EventMsg::ItemCompleted {
                turn_id: self.turn_id.to_string(),
                item_id: tool_item_id.to_string(),
                item: self
                    .runtime
                    .tools
                    .transcript_item_from_result(tool_item_id, call, &result),
            },
        );
        self.runtime.audit().tool_completed(
            self.conversation_id,
            self.turn_id,
            call,
            result.is_error,
            result.content.chars().take(400).collect::<String>(),
        );
        self.record_tool_result(context_manager, result).await
    }

    async fn request_approval<E, F, Fut>(
        &self,
        call: &ToolCall,
        spec: &ToolSpec,
        approval_reason: Option<&str>,
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
                reason: approval_reason
                    .map(ToOwned::to_owned)
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
        self.runtime.audit().approval_requested(
            self.conversation_id,
            self.turn_id,
            call,
            request_reason(approval_reason, &call.name),
            summarize_arguments(&call.arguments),
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
        self.runtime
            .audit()
            .approval_decided(self.conversation_id, self.turn_id, call, &decision);
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
            .unwrap_or_else(|| self.runtime.tools.default_rejection_message(&call.name));
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
            structured: self.runtime.tools.denied_structured_result(
                call.name.as_str(),
                &call.arguments,
                reason.to_string(),
            ),
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
                item: self
                    .runtime
                    .tools
                    .denied_transcript_item(tool_item_id, call, reason),
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
        let result = self.runtime.tools.missing_tool_result(call);
        let message = result.content.clone();
        emit_event(
            events,
            on_event,
            EventMsg::ItemStarted {
                turn_id: self.turn_id.to_string(),
                item_id: tool_item_id.to_string(),
                kind: TurnItemKind::ToolCall,
                title: Some(self.runtime.tools.tool_item_title(call)),
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
        emit_event(
            events,
            on_event,
            EventMsg::ItemCompleted {
                turn_id: self.turn_id.to_string(),
                item_id: tool_item_id.to_string(),
                item: self
                    .runtime
                    .tools
                    .transcript_item_from_result(tool_item_id, call, &result),
            },
        );
        self.record_tool_result(context_manager, result).await
    }

    fn spec_for(&self, call: &ToolCall) -> Option<&ToolSpec> {
        self.tool_specs.iter().find(|spec| spec.name == call.name)
    }

    fn batch_execution_strategy(
        &self,
        ready_calls: &[ReadyToolCall<'_>],
    ) -> ToolBatchExecutionStrategy {
        let calls = ready_calls
            .iter()
            .map(|ready| ready.call.clone())
            .collect::<Vec<_>>();
        self.runtime.tools.batch_execution_strategy(&calls)
    }
}


fn request_reason(approval_reason: Option<&str>, tool_name: &str) -> String {
    approval_reason
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Tool `{tool_name}` requires approval."))
}

