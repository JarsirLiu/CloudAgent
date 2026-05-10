use crate::{GatewayApprovalRequest, GatewayOutbound};
use agent_core::ServerRequest;
use agent_protocol::RequestId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeishuOutboundMessage {
    Text {
        conversation_id: String,
        text: String,
    },
    Card {
        conversation_id: String,
        body: String,
    },
    ApprovalCard {
        conversation_id: String,
        request_id: RequestId,
        body: String,
    },
}

impl FeishuOutboundMessage {
    pub fn conversation_id(&self) -> &str {
        match self {
            Self::Text {
                conversation_id, ..
            }
            | Self::Card {
                conversation_id, ..
            }
            | Self::ApprovalCard {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

impl From<GatewayOutbound> for FeishuOutboundMessage {
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
                body: serde_json::json!({
                    "type": "template",
                    "data": {
                        "template_id": "cloudagent_approval",
                        "template_variable": {
                            "title": approval_title(&request),
                            "description": approval_description(&request),
                        }
                    }
                })
                .to_string(),
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

fn approval_title(request: &ServerRequest) -> &'static str {
    match request {
        ServerRequest::CommandApproval { .. } => "Command approval",
        ServerRequest::FileChangeApproval { .. } => "File change approval",
    }
}

fn approval_description(request: &ServerRequest) -> String {
    match request {
        ServerRequest::CommandApproval { request } => {
            format!("{}\n{}", request.reason, request.command_preview)
        }
        ServerRequest::FileChangeApproval { request } => {
            format!("{}\n{}", request.reason, request.change_preview)
        }
    }
}
