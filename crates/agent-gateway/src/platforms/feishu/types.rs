use serde::Deserialize;

#[derive(Debug, Clone, Default)]
pub struct FeishuBotIdentity {
    pub open_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeishuMessageEnvelope {
    pub sender: FeishuSender,
    pub message: FeishuMessage,
    #[serde(rename = "create_time")]
    pub create_time: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeishuSender {
    #[serde(rename = "sender_id")]
    pub sender_id: Option<FeishuUserId>,
    #[serde(rename = "sender_type")]
    pub sender_type: Option<String>,
    #[serde(rename = "tenant_key")]
    pub tenant_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeishuUserId {
    #[serde(rename = "open_id")]
    pub open_id: Option<String>,
    #[serde(rename = "user_id")]
    pub user_id: Option<String>,
    #[serde(rename = "union_id")]
    pub union_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeishuMessage {
    #[serde(rename = "message_id")]
    pub message_id: Option<String>,
    #[serde(rename = "root_id")]
    pub root_id: Option<String>,
    #[serde(rename = "parent_id")]
    pub parent_id: Option<String>,
    #[serde(rename = "chat_id")]
    pub chat_id: Option<String>,
    #[serde(rename = "chat_type")]
    pub chat_type: Option<String>,
    #[serde(rename = "message_type")]
    pub message_type: Option<String>,
    #[serde(rename = "content")]
    pub content: Option<String>,
    #[serde(rename = "mentions")]
    pub mentions: Option<Vec<FeishuMention>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeishuMention {
    pub key: Option<String>,
    pub name: Option<String>,
    pub id: Option<FeishuUserId>,
}

#[derive(Debug, Clone)]
pub struct NormalizedFeishuMessage {
    pub text: String,
    pub mentioned: bool,
}
