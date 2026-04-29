use agent_protocol::{AppServerNotification, FrontendMode, HistoryEntry, TurnEvent, TurnItemDeltaKind, TurnState};
use agent_runtime::ConversationMessage;

pub(crate) struct ConversationProcessor {
    session_id: String,
    deferred_terminal_notifications: Vec<AppServerNotification>,
}

impl ConversationProcessor {
    pub(crate) fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            deferred_terminal_notifications: Vec::new(),
        }
    }

    pub(crate) fn process_turn_event(
        &mut self,
        event: &TurnEvent,
    ) -> Vec<AppServerNotification> {
        match event {
            TurnEvent::TurnStarted { turn_id, .. } => vec![AppServerNotification::TurnStarted {
                session_id: self.session_id.clone(),
                turn_id: turn_id.clone(),
            }],
            TurnEvent::ItemStarted {
                turn_id,
                item_id,
                kind,
                title,
            } => vec![AppServerNotification::ItemStarted {
                session_id: self.session_id.clone(),
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
                        session_id: self.session_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        kind: kind.clone(),
                        delta: delta.clone(),
                    }]
                }
            }
            TurnEvent::ItemCompleted { turn_id, item, .. } => {
                vec![AppServerNotification::ItemCompleted {
                    session_id: self.session_id.clone(),
                    turn_id: turn_id.clone(),
                    item: item.clone(),
                }]
            }
            TurnEvent::ServerRequestRequested { turn_id, request } => {
                vec![AppServerNotification::ServerRequestRequested {
                    session_id: self.session_id.clone(),
                    turn_id: turn_id.clone(),
                    request: request.clone(),
                }]
            }
            TurnEvent::TurnCompleted { turn_id, .. } => {
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnCompleted {
                        session_id: self.session_id.clone(),
                        turn_id: turn_id.clone(),
                    });
                Vec::new()
            }
            TurnEvent::TurnFailed { turn_id, error } => {
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnFailed {
                        session_id: self.session_id.clone(),
                        turn_id: turn_id.clone(),
                        error: error.clone(),
                    });
                Vec::new()
            }
            TurnEvent::TurnCancelled { turn_id, reason } => {
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnCancelled {
                        session_id: self.session_id.clone(),
                        turn_id: turn_id.clone(),
                        reason: reason.clone(),
                    });
                Vec::new()
            }
            TurnEvent::ServerRequestResolved { .. }
            | TurnEvent::ModelRequestStarted { .. }
            | TurnEvent::ModelResponseReceived { .. } => Vec::new(),
        }
    }

    pub(crate) fn finish_turn(&mut self, turn_state: TurnState) -> Vec<AppServerNotification> {
        let mut notifications = std::mem::take(&mut self.deferred_terminal_notifications);
        if matches!(
            turn_state,
            TurnState::Completed | TurnState::Failed | TurnState::Cancelled
        ) {
            notifications.push(AppServerNotification::FrontendStateChanged {
                session_id: self.session_id.clone(),
                mode: FrontendMode::Idle,
            });
        }
        notifications
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

#[cfg(test)]
mod tests {
    use super::ConversationProcessor;
    use agent_protocol::{AppServerNotification, FrontendMode, TurnEvent, TurnState};

    #[test]
    fn terminal_notifications_are_deferred_until_finish() {
        let mut processor = ConversationProcessor::new("default");

        let immediate = processor.process_turn_event(&TurnEvent::TurnCompleted {
            turn_id: "turn-1".to_string(),
            final_response: "done".to_string(),
        });
        assert!(immediate.is_empty());

        let flushed = processor.finish_turn(TurnState::Completed);
        assert_eq!(flushed.len(), 2);
        assert!(matches!(
            &flushed[0],
            AppServerNotification::TurnCompleted { turn_id, .. } if turn_id == "turn-1"
        ));
        assert!(matches!(
            &flushed[1],
            AppServerNotification::FrontendStateChanged { mode: FrontendMode::Idle, .. }
        ));
    }
}
