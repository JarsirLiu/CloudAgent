use crate::conversation::{ConversationHistory, ResponseItem};

#[derive(Clone, Debug, Default)]
pub struct ConversationMemory {
    pub last_user_message: Option<String>,
    pub last_assistant_message: Option<String>,
}

impl ConversationMemory {
    pub fn from_history(history: &ConversationHistory) -> Self {
        let mut memory = Self::default();
        for message in &history.messages {
            match message {
                ResponseItem::User { content } => {
                    memory.last_user_message = Some(content.clone());
                }
                ResponseItem::Assistant { content, .. } => {
                    if let Some(content) = content {
                        memory.last_assistant_message = Some(content.clone());
                    }
                }
                ResponseItem::System { .. } | ResponseItem::Tool { .. } => {}
            }
        }
        memory
    }
}

pub fn module_name() -> &'static str {
    "agent-core::memory"
}
