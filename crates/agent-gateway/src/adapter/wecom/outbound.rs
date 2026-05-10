use crate::{GatewayApprovalRequest, GatewayOutbound};
use agent_core::ServerRequest;
use agent_protocol::RequestId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WecomOutboundMessage {
    Text {
        conversation_id: String,
        text: String,
    },
    ApprovalCard {
        conversation_id: String,
        request_id: RequestId,
        body: String,
    },
}

impl WecomOutboundMessage {
    pub fn conversation_id(&self) -> &str {
        match self {
            Self::Text {
                conversation_id, ..
            }
            | Self::ApprovalCard {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

impl From<GatewayOutbound> for WecomOutboundMessage {
    fn from(outbound: GatewayOutbound) -> Self {
        match outbound {
            GatewayOutbound::TextDelta {
                conversation_id,
                delta,
            } => Self::Text {
                conversation_id,
                text: delta,
            },
            GatewayOutbound::ApprovalRequest(GatewayApprovalRequest {
                conversation_id,
                request_id,
                request,
            }) => Self::ApprovalCard {
                conversation_id,
                request_id,
                body: approval_description(&request),
            },
            GatewayOutbound::ToolNotice {
                conversation_id,
                message,
            }
            | GatewayOutbound::Info {
                conversation_id,
                message,
            }
            | GatewayOutbound::Error {
                conversation_id,
                message,
            } => Self::Text {
                conversation_id,
                text: message,
            },
        }
    }
}

fn approval_description(request: &ServerRequest) -> String {
    match request {
        ServerRequest::CommandApproval { request } => {
            format!(
                "Approval required\n{}\n{}",
                request.reason, request.command_preview
            )
        }
        ServerRequest::FileChangeApproval { request } => {
            format!(
                "Approval required\n{}\n{}",
                request.reason, request.change_preview
            )
        }
    }
}
