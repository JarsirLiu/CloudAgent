use crate::conversation::ConversationMessage;
use agent_protocol::HistoryEntry;

pub fn history_entry_from_message(message: &ConversationMessage) -> HistoryEntry {
    match message {
        ConversationMessage::System { content } => HistoryEntry::System {
            content: content.clone(),
        },
        ConversationMessage::User { content } => HistoryEntry::User {
            content: content.clone(),
        },
        ConversationMessage::Assistant {
            content,
            tool_calls,
        } => HistoryEntry::Assistant {
            content: content.clone(),
            has_tool_calls: !tool_calls.is_empty(),
        },
        ConversationMessage::Tool {
            tool_call_id,
            name,
            content,
            structured,
        } => HistoryEntry::Tool {
            tool_call_id: tool_call_id.clone(),
            name: name.clone(),
            content: content.clone(),
            structured: structured.clone(),
        },
    }
}
