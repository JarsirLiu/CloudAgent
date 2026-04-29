use agent_protocol::{AppServerNotification, FrontendMode, TurnEvent, TurnItemDeltaKind, TurnState};

pub(crate) struct ConversationNotificationProjector {
    conversation_id: String,
    deferred_terminal_notifications: Vec<AppServerNotification>,
}

impl ConversationNotificationProjector {
    pub(crate) fn new(conversation_id: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            deferred_terminal_notifications: Vec::new(),
        }
    }

    pub(crate) fn project_turn_event(&mut self, event: &TurnEvent) -> Vec<AppServerNotification> {
        match event {
            TurnEvent::TurnStarted { turn_id, .. } => vec![AppServerNotification::TurnStarted {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
            }],
            TurnEvent::ItemStarted {
                turn_id,
                item_id,
                kind,
                title,
            } => vec![AppServerNotification::ItemStarted {
                conversation_id: self.conversation_id.clone(),
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
            } => match kind {
                TurnItemDeltaKind::Text => vec![AppServerNotification::AgentMessageDelta {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                }],
                TurnItemDeltaKind::ReasoningSummary => {
                    vec![AppServerNotification::ReasoningSummaryTextDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::ReasoningText => {
                    vec![AppServerNotification::ReasoningTextDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::ToolOutput => {
                    vec![AppServerNotification::CommandExecutionOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::JsonPatch => Vec::new(),
            },
            TurnEvent::ItemCompleted { turn_id, item, .. } => {
                vec![AppServerNotification::ItemCompleted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: turn_id.clone(),
                    item: item.clone(),
                }]
            }
            TurnEvent::ServerRequestRequested { turn_id, request } => {
                vec![AppServerNotification::ServerRequestRequested {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: turn_id.clone(),
                    request: request.clone(),
                }]
            }
            TurnEvent::TurnCompleted { turn_id, .. } => {
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnCompleted {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                    });
                Vec::new()
            }
            TurnEvent::TurnFailed { turn_id, error } => {
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnFailed {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        error: error.clone(),
                    });
                Vec::new()
            }
            TurnEvent::TurnCancelled { turn_id, reason } => {
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnCancelled {
                        conversation_id: self.conversation_id.clone(),
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
                conversation_id: self.conversation_id.clone(),
                mode: FrontendMode::Idle,
            });
        }
        notifications
    }
}

#[cfg(test)]
mod tests {
    use super::ConversationNotificationProjector;
    use agent_protocol::{AppServerNotification, FrontendMode, TurnEvent, TurnState};

    #[test]
    fn terminal_notifications_are_deferred_until_finish() {
        let mut projector = ConversationNotificationProjector::new("default");

        let immediate = projector.project_turn_event(&TurnEvent::TurnCompleted {
            turn_id: "turn-1".to_string(),
            final_response: "done".to_string(),
        });
        assert!(immediate.is_empty());

        let flushed = projector.finish_turn(TurnState::Completed);
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
