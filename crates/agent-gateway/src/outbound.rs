use agent_core::ServerRequest;
use agent_protocol::RequestId;

#[derive(Clone, Debug)]
pub struct GatewayApprovalRequest {
    pub conversation_id: String,
    pub request_id: RequestId,
    pub request: ServerRequest,
}

#[derive(Clone, Debug)]
pub enum GatewayProgressKind {
    Plan,
    Reasoning,
    Tool,
}

#[derive(Clone, Debug)]
pub struct GatewayProgressUpdate {
    pub conversation_id: String,
    pub kind: GatewayProgressKind,
    pub summary: String,
    pub streaming: bool,
}

#[derive(Clone, Debug)]
pub enum GatewayOutbound {
    TextDelta {
        conversation_id: String,
        delta: String,
    },
    FlushText {
        conversation_id: String,
    },
    FinalText {
        conversation_id: String,
        text: String,
    },
    ApprovalRequest(GatewayApprovalRequest),
    Progress(GatewayProgressUpdate),
    Info {
        conversation_id: String,
        message: String,
    },
    Error {
        conversation_id: String,
        message: String,
    },
}
