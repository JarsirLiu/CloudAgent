use crate::host::{AgentHost, AgentHostExt};
use crate::model::await_server_request_decision;
use crate::tool::{ToolCall, ToolSpec};
use crate::turn::{
    CommandApprovalRequest, EventMsg, FileChangeApprovalRequest, ServerRequest,
    ServerRequestDecisionKind, ServerRequestHandler, TurnHost, TurnItemKind, TurnState, emit_event,
};
use crate::{ApprovalGrantKey, ApprovalPolicy, PermissionProfile};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

pub(crate) enum ApprovalFlow {
    Approved,
    Denied { reason: String },
    Cancelled,
}

pub(crate) struct ApprovalRuntime<'a> {
    host: &'a AgentHost,
    conversation_id: &'a str,
    turn_id: &'a str,
    #[allow(dead_code)]
    permission_profile: &'a PermissionProfile,
    #[allow(dead_code)]
    approval_policy: &'a ApprovalPolicy,
    cancellation_token: &'a CancellationToken,
    approval: &'a dyn ServerRequestHandler,
}

impl<'a> ApprovalRuntime<'a> {
    pub(crate) fn new(
        host: &'a AgentHost,
        conversation_id: &'a str,
        turn_id: &'a str,
        permission_profile: &'a PermissionProfile,
        approval_policy: &'a ApprovalPolicy,
        cancellation_token: &'a CancellationToken,
        approval: &'a dyn ServerRequestHandler,
    ) -> Self {
        Self {
            host,
            conversation_id,
            turn_id,
            permission_profile,
            approval_policy,
            cancellation_token,
            approval,
        }
    }

    pub(crate) async fn has_session_grant(&self, key: &ApprovalGrantKey) -> bool {
        self.host
            .approval_grants()
            .has_approval_grant(self.conversation_id, key)
            .await
            .unwrap_or(false)
    }

    pub(crate) async fn authorize_tool_call(
        &self,
        call: &ToolCall,
        spec: &ToolSpec,
        approval_grant_key: Option<&ApprovalGrantKey>,
        approval_reason: Option<&str>,
        events: &mut Vec<EventMsg>,
        on_event: &mut (dyn for<'b> FnMut(&'b EventMsg) + Send + '_),
    ) -> Result<ApprovalFlow> {
        self.host
            .state()
            .update_turn_state(
                self.conversation_id,
                self.turn_id,
                TurnState::WaitingForServerRequest,
            )
            .await;
        let request = approval_request_for_call(
            self.turn_id,
            call,
            spec,
            approval_reason
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("Tool `{}` requires approval.", call.name)),
        );
        emit_event(
            events,
            on_event,
            EventMsg::ServerRequestRequested {
                turn_id: self.turn_id.to_string(),
                request: request.clone(),
            },
        );
        self.host.audit_tool_approval_requested(
            self.conversation_id,
            self.turn_id,
            call,
            request_reason(approval_reason, &call.name),
        );
        let decision = await_server_request_decision(
            self.cancellation_token,
            self.approval.decide(request.clone()),
            self.host.turn_interrupted_error(),
        )
        .await?;
        self.host
            .state()
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
        self.host
            .audit_tool_approval_decided(self.conversation_id, self.turn_id, call, &decision);

        if decision.is_approved() {
            if matches!(
                decision.decision,
                ServerRequestDecisionKind::AcceptForSession
            ) && let Some(key) = approval_grant_key
            {
                let _ = self
                    .host
                    .approval_grants()
                    .save_approval_grant(self.conversation_id, key)
                    .await;
            }
            return Ok(ApprovalFlow::Approved);
        }

        if matches!(decision.decision, ServerRequestDecisionKind::Cancel) {
            return Ok(ApprovalFlow::Cancelled);
        }

        let reason = decision
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| self.host.tools().default_rejection_message(&call.name));
        Ok(ApprovalFlow::Denied { reason })
    }
}

fn approval_request_for_call(
    turn_id: &str,
    call: &ToolCall,
    spec: &ToolSpec,
    reason: String,
) -> ServerRequest {
    let preview = crate::tool::summarize_tool_arguments(&call.name, &call.arguments);
    match spec.item_kind {
        TurnItemKind::CommandExecution => ServerRequest::CommandApproval {
            request: CommandApprovalRequest {
                turn_id: turn_id.to_string(),
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                reason,
                command_preview: preview,
            },
        },
        TurnItemKind::FileChange => ServerRequest::FileChangeApproval {
            request: FileChangeApprovalRequest {
                turn_id: turn_id.to_string(),
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                reason,
                change_preview: preview,
            },
        },
        _ => ServerRequest::FileChangeApproval {
            request: FileChangeApprovalRequest {
                turn_id: turn_id.to_string(),
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                reason,
                change_preview: preview,
            },
        },
    }
}

fn request_reason(approval_reason: Option<&str>, tool_name: &str) -> String {
    approval_reason
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Tool `{tool_name}` requires approval."))
}
