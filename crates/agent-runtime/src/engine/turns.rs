use crate::{AgentRuntime, MANUAL_COMPACTION_MIN_HISTORY_TOKENS, ManualCompactionOutcome, tasks};
use agent_core::AgentTurnOutput;
use agent_protocol::{EventMsg, RequestId, ServerRequest, ServerRequestDecision};
use anyhow::{Result, bail};
use tokio_util::sync::CancellationToken;

impl AgentRuntime {
    pub async fn compact_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ManualCompactionOutcome> {
        self.rollout_recorder.flush().await?;
        tasks::run_manual_compaction(self, conversation_id, MANUAL_COMPACTION_MIN_HISTORY_TOKENS)
            .await
    }

    pub async fn chat(&self, conversation_id: &str, user_input: &str) -> Result<AgentTurnOutput> {
        let outcome = self
            .chat_with_approval_and_events(
                conversation_id,
                user_input,
                "safe",
                |_event| {},
                |_request| async move {
                    Ok(ServerRequestDecision::decline(Some(
                        "Mutating tools require an approval-capable client. Use the interactive cli."
                            .to_string(),
                    )))
                },
            )
            .await?;
        Ok(outcome)
    }

    pub async fn chat_with_approval<F, Fut>(
        &self,
        conversation_id: &str,
        user_input: &str,
        permission_mode: &str,
        approval: F,
    ) -> Result<AgentTurnOutput>
    where
        F: Fn(ServerRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
    {
        self.chat_with_approval_and_events(
            conversation_id,
            user_input,
            permission_mode,
            |_event| {},
            approval,
        )
            .await
    }

    pub async fn chat_with_approval_and_events<E, F, Fut>(
        &self,
        conversation_id: &str,
        user_input: &str,
        permission_mode: &str,
        mut on_event: E,
        approval: F,
    ) -> Result<AgentTurnOutput>
    where
        E: FnMut(&EventMsg) + Send,
        F: Fn(ServerRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
    {
        self.audit().turn_started(conversation_id, user_input);
        let outcome = super::run_turn_with_approval(
            self,
            conversation_id,
            user_input,
            permission_mode,
            &mut on_event,
            approval,
        )
        .await?;
        self.audit().turn_completed(
            conversation_id,
            &outcome.turn_id,
            &format!("{:?}", outcome.state),
            outcome.events.len(),
            outcome.model_name.as_deref(),
        );
        Ok(self.outcome_to_output(outcome))
    }

    pub async fn interrupt_conversation(&self, conversation_id: &str) -> bool {
        self.state.interrupt_conversation(conversation_id).await
    }

    pub async fn register_pending_request(
        &self,
        conversation_id: &str,
        request_id: RequestId,
        request: ServerRequest,
    ) {
        self.state
            .set_pending_request(conversation_id, request_id, request)
            .await;
    }

    pub async fn resolve_pending_request(&self, conversation_id: &str, request_id: &RequestId) {
        self.state
            .resolve_pending_request(conversation_id, request_id)
            .await;
    }

    pub(crate) async fn complete_model_request_streaming(
        &self,
        cancellation_token: &CancellationToken,
        request: agent_core::ModelRequest,
        on_text_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<agent_core::ModelResponse> {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                bail!(crate::TURN_INTERRUPTED_ERROR);
            }
            response = self.model.complete_streaming(request, on_text_delta) => response,
        }
    }

    pub(crate) async fn complete_model_request(
        &self,
        cancellation_token: &CancellationToken,
        request: agent_core::ModelRequest,
    ) -> Result<agent_core::ModelResponse> {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                bail!(crate::TURN_INTERRUPTED_ERROR);
            }
            response = self.model.complete(request) => response,
        }
    }

    pub(crate) async fn await_approval<Fut>(
        &self,
        cancellation_token: &CancellationToken,
        approval_future: Fut,
    ) -> Result<ServerRequestDecision>
    where
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
    {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                bail!(crate::TURN_INTERRUPTED_ERROR);
            }
            response = approval_future => response,
        }
    }

    pub(crate) async fn execute_tool_call_streaming<F>(
        &self,
        cancellation_token: &CancellationToken,
        call: agent_core::ToolCall,
        ctx: &agent_core::ToolExecutionContext,
        mut on_output_delta: F,
    ) -> Result<agent_core::ToolResult>
    where
        F: FnMut(agent_core::ToolOutputDelta) + Send,
    {
        let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel();
        let streaming_ctx = ctx.clone().with_output_tx(output_tx);
        let execution = self.tools.execute(call, &streaming_ctx);
        tokio::pin!(execution);

        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    bail!(crate::TURN_INTERRUPTED_ERROR);
                }
                Some(delta) = output_rx.recv() => {
                    on_output_delta(delta);
                }
                response = &mut execution => {
                    while let Ok(delta) = output_rx.try_recv() {
                        on_output_delta(delta);
                    }
                    return response;
                }
            }
        }
    }

    fn outcome_to_output(&self, outcome: crate::tasks::TurnOutcome) -> AgentTurnOutput {
        agent_core::agent_turn_output_from_events(
            outcome.turn_id,
            outcome.events,
            &outcome.history,
            outcome.model_name,
            outcome.state,
        )
    }
}
