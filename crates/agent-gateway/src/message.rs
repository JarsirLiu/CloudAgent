use agent_core::InputItem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayMessage {
    pub conversation_id: String,
    pub sender_id: String,
    pub content: Vec<InputItem>,
}

impl GatewayMessage {
    pub fn new(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        content: Vec<InputItem>,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            sender_id: sender_id.into(),
            content,
        }
    }
}
