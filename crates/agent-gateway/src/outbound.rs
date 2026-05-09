use agent_core::ServerRequest;
use agent_protocol::RequestId;

#[derive(Clone, Debug)]
pub struct GatewayApprovalRequest {
    pub conversation_id: String,
    pub request_id: RequestId,
    pub request: ServerRequest,
}

#[derive(Clone, Debug)]
pub enum GatewayOutbound {
    TextDelta {
        conversation_id: String,
        delta: String,
    },
    ApprovalRequest(GatewayApprovalRequest),
    ToolNotice {
        conversation_id: String,
        message: String,
    },
    Info {
        conversation_id: String,
        message: String,
    },
    Error {
        conversation_id: String,
        message: String,
    },
}
