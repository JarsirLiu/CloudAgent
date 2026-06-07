use agent_core::ServerRequest;
use agent_protocol::{PendingServerRequestView, RequestId, ServerRequestViewKind};

pub(crate) fn pending_request_view(
    conversation_id: &str,
    request_id: RequestId,
    request: &ServerRequest,
    created_at_ms: u64,
) -> PendingServerRequestView {
    match request {
        ServerRequest::CommandApproval { request } => PendingServerRequestView {
            request_id,
            conversation_id: conversation_id.to_string(),
            turn_id: request.turn_id.clone(),
            kind: ServerRequestViewKind::CommandApproval,
            tool_name: request.tool_name.clone(),
            reason: request.reason.clone(),
            preview: request.command_preview.clone(),
            created_at_ms,
        },
        ServerRequest::FileChangeApproval { request } => PendingServerRequestView {
            request_id,
            conversation_id: conversation_id.to_string(),
            turn_id: request.turn_id.clone(),
            kind: ServerRequestViewKind::FileChangeApproval,
            tool_name: request.tool_name.clone(),
            reason: request.reason.clone(),
            preview: request.change_preview.clone(),
            created_at_ms,
        },
    }
}
