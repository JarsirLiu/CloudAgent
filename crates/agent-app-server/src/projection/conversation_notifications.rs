use agent_core::{CoreTranscriptEvent, core_transcript_event_from_event_msg};
use agent_protocol::{AppServerNotification, EventMsg, FrontendMode, TurnItemDeltaKind, TurnState};

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

    pub(crate) fn project_turn_event(&mut self, event: &EventMsg) -> Vec<AppServerNotification> {
        if let Some(core_event) = core_transcript_event_from_event_msg(event) {
            return self.project_core_transcript_event(core_event);
        }

        match event {
            EventMsg::TurnStarted { turn_id, .. } => vec![AppServerNotification::TurnStarted {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
            }],
            EventMsg::ItemStarted {
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
            EventMsg::ItemDelta {
                turn_id,
                item_id,
                kind,
                delta,
            } => match kind {
                TurnItemDeltaKind::Text
                | TurnItemDeltaKind::ReasoningSummary
                | TurnItemDeltaKind::ReasoningText => Vec::new(),
                TurnItemDeltaKind::ToolOutput => {
                    vec![AppServerNotification::CommandExecutionOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::FileChangeOutput => {
                    vec![AppServerNotification::FileChangeOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::JsonPatch => Vec::new(),
            },
            EventMsg::ItemCompleted { .. } => Vec::new(),
            EventMsg::ServerRequestRequested { turn_id, request } => {
                vec![AppServerNotification::ServerRequestRequested {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: turn_id.clone(),
                    request: request.clone(),
                }]
            }
            EventMsg::TurnCompleted { .. } => Vec::new(),
            EventMsg::TurnFailed { turn_id, error } => {
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnFailed {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        error: error.clone(),
                    });
                Vec::new()
            }
            EventMsg::TurnCancelled { turn_id, reason } => {
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnCancelled {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        reason: reason.clone(),
                    });
                Vec::new()
            }
            EventMsg::ServerRequestResolved { .. }
            | EventMsg::ModelRequestStarted { .. }
            | EventMsg::ModelResponseReceived { .. } => Vec::new(),
        }
    }

    fn project_core_transcript_event(
        &mut self,
        event: CoreTranscriptEvent,
    ) -> Vec<AppServerNotification> {
        match event {
            CoreTranscriptEvent::TurnCompleted { turn_id } => {
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnCompleted {
                        conversation_id: self.conversation_id.clone(),
                        turn_id,
                    });
                Vec::new()
            }
            CoreTranscriptEvent::ItemCompleted { turn_id, item } => {
                vec![AppServerNotification::ItemCompleted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id,
                    item,
                }]
            }
            CoreTranscriptEvent::AgentMessageDelta {
                turn_id,
                item_id,
                delta,
            } => vec![AppServerNotification::AgentMessageDelta {
                conversation_id: self.conversation_id.clone(),
                turn_id,
                item_id,
                delta,
            }],
            CoreTranscriptEvent::PlanDelta {
                turn_id,
                item_id,
                delta,
            } => vec![AppServerNotification::PlanDelta {
                conversation_id: self.conversation_id.clone(),
                turn_id,
                item_id,
                delta,
            }],
            CoreTranscriptEvent::ReasoningSummaryTextDelta {
                turn_id,
                item_id,
                delta,
            } => vec![AppServerNotification::ReasoningSummaryTextDelta {
                conversation_id: self.conversation_id.clone(),
                turn_id,
                item_id,
                delta,
            }],
            CoreTranscriptEvent::ReasoningTextDelta {
                turn_id,
                item_id,
                delta,
            } => vec![AppServerNotification::ReasoningTextDelta {
                conversation_id: self.conversation_id.clone(),
                turn_id,
                item_id,
                delta,
            }],
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
    use agent_protocol::{
        AppServerNotification, EventMsg, FrontendMode, TurnItemDeltaKind, TurnState,
    };

    #[test]
    fn terminal_notifications_are_deferred_until_finish() {
        let mut projector = ConversationNotificationProjector::new("default");

        let immediate = projector.project_turn_event(&EventMsg::TurnCompleted {
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
            AppServerNotification::FrontendStateChanged {
                mode: FrontendMode::Idle,
                ..
            }
        ));
    }

    #[test]
    fn file_change_output_delta_projects_to_file_change_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:write".to_string(),
            kind: TurnItemDeltaKind::FileChangeOutput,
            delta: "wrote note.txt".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::FileChangeOutputDelta { item_id, delta, .. }
                if item_id == "tool:write" && delta == "wrote note.txt"
        ));
    }
}
