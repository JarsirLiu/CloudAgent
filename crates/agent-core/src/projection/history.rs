use crate::conversation::{ResponseItem, TranscriptItem};

pub fn history_entry_from_message(message: &ResponseItem) -> TranscriptItem {
    match message {
        ResponseItem::System { content } => TranscriptItem::SystemMessage {
            id: "system".to_string(),
            text: content.clone(),
        },
        ResponseItem::User { content } => TranscriptItem::UserMessage {
            id: String::new(),
            text: content.clone(),
        },
        ResponseItem::Assistant {
            content,
            tool_calls: _,
        } => TranscriptItem::AgentMessage {
            id: String::new(),
            text: content.clone().unwrap_or_default(),
        },
        ResponseItem::Tool {
            tool_call_id,
            name,
            content,
            structured,
        } => TranscriptItem::ToolResult {
            id: tool_call_id.clone(),
            tool_name: name.clone(),
            content: content.clone(),
            summary: content.clone(),
            structured: structured.clone(),
        },
    }
}
