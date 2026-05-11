#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub platform: String,
    pub tenant_key: Option<String>,
    pub chat_id: String,
    pub chat_type: Option<String>,
    pub sender_open_id: String,
    pub sender_user_id: Option<String>,
    pub sender_union_id: Option<String>,
    pub message_id: String,
    pub thread_id: Option<String>,
    pub text: String,
    pub mentioned: bool,
    pub reply_context: Option<ReplyContext>,
}

#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub chat_id: String,
    pub text: String,
    pub is_group_context: bool,
    pub reply_context: Option<ReplyContext>,
}

#[derive(Debug, Clone)]
pub struct ReplyContext {
    pub message_id: String,
    pub thread_id: Option<String>,
}
