use agent_protocol::{AppServerNotification, HistoryEntry, TurnEvent, TurnItemDeltaKind};
use agent_runtime::ConversationMessage;

pub(crate) fn project_turn_event(
    session_id: &str,
    event: &TurnEvent,
) -> Vec<AppServerNotification> {
    match event {
        TurnEvent::TurnStarted { turn_id, .. } => vec![AppServerNotification::TurnStarted {
            session_id: session_id.to_string(),
            turn_id: turn_id.clone(),
        }],
        TurnEvent::ItemStarted {
            turn_id,
            item_id,
            kind,
            title,
        } => vec![AppServerNotification::ItemStarted {
            session_id: session_id.to_string(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            kind: kind.clone(),
            title: title.clone(),
        }],
        TurnEvent::ItemDelta {
            turn_id,
            item_id,
            kind,
            delta,
        } => {
            if matches!(kind, TurnItemDeltaKind::JsonPatch) {
                Vec::new()
            } else {
                vec![AppServerNotification::ItemDelta {
                    session_id: session_id.to_string(),
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    kind: kind.clone(),
                    delta: delta.clone(),
                }]
            }
        }
        TurnEvent::ItemCompleted { turn_id, item, .. } => vec![AppServerNotification::ItemCompleted {
            session_id: session_id.to_string(),
            turn_id: turn_id.clone(),
            item: item.clone(),
        }],
        TurnEvent::TurnCompleted { turn_id, .. } => vec![
            AppServerNotification::TurnCompleted {
                session_id: session_id.to_string(),
                turn_id: turn_id.clone(),
            },
            AppServerNotification::FrontendStateChanged {
                session_id: session_id.to_string(),
                mode: agent_protocol::FrontendMode::Idle,
            },
        ],
        TurnEvent::TurnFailed { turn_id, error } => vec![
            AppServerNotification::TurnFailed {
                session_id: session_id.to_string(),
                turn_id: turn_id.clone(),
                error: error.clone(),
            },
            AppServerNotification::FrontendStateChanged {
                session_id: session_id.to_string(),
                mode: agent_protocol::FrontendMode::Idle,
            },
        ],
        TurnEvent::TurnCancelled { turn_id, reason } => vec![
            AppServerNotification::TurnCancelled {
                session_id: session_id.to_string(),
                turn_id: turn_id.clone(),
                reason: reason.clone(),
            },
            AppServerNotification::FrontendStateChanged {
                session_id: session_id.to_string(),
                mode: agent_protocol::FrontendMode::Idle,
            },
        ],
        TurnEvent::ServerRequestRequested { turn_id, request } => {
            vec![AppServerNotification::ServerRequestRequested {
                session_id: session_id.to_string(),
                turn_id: turn_id.clone(),
                request: request.clone(),
            }]
        }
        TurnEvent::ServerRequestResolved {
            ..
        } => Vec::new(),
        TurnEvent::ModelRequestStarted { .. }
        | TurnEvent::ModelResponseReceived { .. } => Vec::new(),
    }
}

pub(crate) fn history_entry_from_message(message: &ConversationMessage) -> HistoryEntry {
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
