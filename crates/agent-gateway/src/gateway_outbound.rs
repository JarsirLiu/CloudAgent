use crate::message::ReplyContext;

#[derive(Debug, Clone)]
pub struct OutboundTarget {
    pub conversation_id: String,
    pub chat_id: String,
    pub reply_context: Option<ReplyContext>,
}

#[derive(Debug, Clone)]
pub enum GatewayProgressKind {
    Plan,
    Reasoning,
    Tool,
}

#[derive(Debug, Clone)]
pub struct GatewayProgressUpdate {
    pub target: OutboundTarget,
    pub kind: GatewayProgressKind,
    pub summary: String,
    pub streaming: bool,
}

#[derive(Debug, Clone)]
pub enum GatewayOutbound {
    TextDelta {
        target: OutboundTarget,
        delta: String,
    },
    FlushText {
        target: OutboundTarget,
    },
    FinalText {
        target: OutboundTarget,
        text: String,
    },
    Progress(GatewayProgressUpdate),
    Info {
        target: OutboundTarget,
        message: String,
    },
    Error {
        target: OutboundTarget,
        message: String,
    },
}
