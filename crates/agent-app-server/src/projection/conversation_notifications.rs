use agent_core::{CoreTranscriptEvent, core_transcript_event_from_event_msg};
use agent_protocol::{AppServerNotification, EventMsg, FrontendMode, TurnItemDeltaKind, TurnState};
use std::collections::HashMap;

pub(crate) struct ConversationNotificationProjector {
    conversation_id: String,
    deferred_terminal_notifications: Vec<AppServerNotification>,
    active_items: HashMap<String, String>,
}

impl ConversationNotificationProjector {
    pub(crate) fn new(conversation_id: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            deferred_terminal_notifications: Vec::new(),
            active_items: HashMap::new(),
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
            } => {
                self.active_items.insert(item_id.clone(), turn_id.clone());
                vec![AppServerNotification::ItemStarted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    kind: kind.clone(),
                    title: title.clone(),
                }]
            }
            EventMsg::ItemDelta {
                turn_id,
                item_id,
                kind,
                delta,
            } => match kind {
                TurnItemDeltaKind::Text
                | TurnItemDeltaKind::ReasoningSummary
                | TurnItemDeltaKind::ReasoningText => Vec::new(),
                TurnItemDeltaKind::CommandExecutionOutput => {
                    if let Some(error) = self.validate_active_item(turn_id, item_id) {
                        return vec![error];
                    }
                    vec![AppServerNotification::CommandExecutionOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::ToolOutput => {
                    if let Some(error) = self.validate_active_item(turn_id, item_id) {
                        return vec![error];
                    }
                    vec![AppServerNotification::ToolOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::FileChangeOutput => {
                    if let Some(error) = self.validate_active_item(turn_id, item_id) {
                        return vec![error];
                    }
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
            EventMsg::TokenUsageUpdated {
                turn_id,
                last_usage,
                total_usage,
                model_context_window,
            } => vec![AppServerNotification::TokenUsageUpdated {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
                last_usage: last_usage.clone(),
                total_usage: total_usage.clone(),
                model_context_window: *model_context_window,
            }],
            EventMsg::ContextCompacted {
                turn_id,
                pre_context_tokens_estimate,
                post_context_tokens_estimate,
                pre_message_count,
                post_message_count,
                preserved_tail_count,
            } => vec![AppServerNotification::ContextCompacted {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
                pre_context_tokens_estimate: *pre_context_tokens_estimate,
                post_context_tokens_estimate: *post_context_tokens_estimate,
                pre_message_count: *pre_message_count,
                post_message_count: *post_message_count,
                preserved_tail_count: *preserved_tail_count,
            }],
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
                if let Some(error) = self.active_items_not_closed_error(&turn_id) {
                    self.deferred_terminal_notifications.push(error);
                }
                self.active_items
                    .retain(|_, active_turn_id| active_turn_id != &turn_id);
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnCompleted {
                        conversation_id: self.conversation_id.clone(),
                        turn_id,
                    });
                Vec::new()
            }
            CoreTranscriptEvent::ItemCompleted { turn_id, item } => {
                let item_id = item.id().to_string();
                let lifecycle_error = self.validate_active_item(&turn_id, &item_id);
                self.active_items.remove(&item_id);
                let mut notifications = Vec::new();
                if let Some(error) = lifecycle_error {
                    notifications.push(error);
                }
                notifications.push(AppServerNotification::ItemCompleted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id,
                    item,
                });
                notifications
            }
            CoreTranscriptEvent::AgentMessageDelta {
                turn_id,
                item_id,
                delta,
            } => self.project_core_delta(
                turn_id,
                item_id,
                delta,
                |conversation_id, turn_id, item_id, delta| {
                    AppServerNotification::AgentMessageDelta {
                        conversation_id,
                        turn_id,
                        item_id,
                        delta,
                    }
                },
            ),
            CoreTranscriptEvent::PlanDelta {
                turn_id,
                item_id,
                delta,
            } => self.project_core_delta(
                turn_id,
                item_id,
                delta,
                |conversation_id, turn_id, item_id, delta| AppServerNotification::PlanDelta {
                    conversation_id,
                    turn_id,
                    item_id,
                    delta,
                },
            ),
            CoreTranscriptEvent::ReasoningSummaryTextDelta {
                turn_id,
                item_id,
                delta,
            } => self.project_core_delta(
                turn_id,
                item_id,
                delta,
                |conversation_id, turn_id, item_id, delta| {
                    AppServerNotification::ReasoningSummaryTextDelta {
                        conversation_id,
                        turn_id,
                        item_id,
                        delta,
                    }
                },
            ),
            CoreTranscriptEvent::ReasoningTextDelta {
                turn_id,
                item_id,
                delta,
            } => self.project_core_delta(
                turn_id,
                item_id,
                delta,
                |conversation_id, turn_id, item_id, delta| {
                    AppServerNotification::ReasoningTextDelta {
                        conversation_id,
                        turn_id,
                        item_id,
                        delta,
                    }
                },
            ),
        }
    }

    fn project_core_delta(
        &self,
        turn_id: String,
        item_id: String,
        delta: String,
        build: impl FnOnce(String, String, String, String) -> AppServerNotification,
    ) -> Vec<AppServerNotification> {
        if let Some(error) = self.validate_active_item(&turn_id, &item_id) {
            return vec![error];
        }
        vec![build(self.conversation_id.clone(), turn_id, item_id, delta)]
    }

    fn validate_active_item(&self, turn_id: &str, item_id: &str) -> Option<AppServerNotification> {
        match self.active_items.get(item_id) {
            Some(active_turn_id) if active_turn_id == turn_id => None,
            Some(active_turn_id) => Some(self.lifecycle_error(format!(
                "item `{item_id}` belongs to turn `{active_turn_id}` but received event for turn `{turn_id}`"
            ))),
            None => Some(self.lifecycle_error(format!(
                "item `{item_id}` received lifecycle event before item start"
            ))),
        }
    }

    fn active_items_not_closed_error(&self, turn_id: &str) -> Option<AppServerNotification> {
        let dangling = self
            .active_items
            .iter()
            .filter_map(|(item_id, active_turn_id)| {
                (active_turn_id == turn_id).then(|| item_id.as_str())
            })
            .collect::<Vec<_>>();
        (!dangling.is_empty()).then(|| {
            self.lifecycle_error(format!(
                "turn `{turn_id}` completed with active items: {}",
                dangling.join(", ")
            ))
        })
    }

    fn lifecycle_error(&self, message: String) -> AppServerNotification {
        AppServerNotification::Error {
            conversation_id: self.conversation_id.clone(),
            message,
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
        AppServerNotification, EventMsg, FrontendMode, TranscriptItem, TurnItemDeltaKind,
        TurnItemKind, TurnState,
    };

    #[test]
    fn terminal_notifications_are_deferred_until_finish() {
        let mut projector = ConversationNotificationProjector::new("default");

        let immediate = projector.project_turn_event(&EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
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

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:write".to_string(),
            kind: TurnItemKind::FileChange,
            title: Some("apply_patch".to_string()),
        });
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

    #[test]
    fn command_execution_output_delta_projects_to_command_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            kind: TurnItemKind::CommandExecution,
            title: Some("shell_command".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            delta: "D:\\work".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::CommandExecutionOutputDelta { item_id, delta, .. }
                if item_id == "tool:shell" && delta == "D:\\work"
        ));
    }

    #[test]
    fn generic_tool_output_delta_projects_to_tool_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:custom".to_string(),
            kind: TurnItemKind::ToolCall,
            title: Some("custom_tool".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:custom".to_string(),
            kind: TurnItemDeltaKind::ToolOutput,
            delta: "custom output".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::ToolOutputDelta { item_id, delta, .. }
                if item_id == "tool:custom" && delta == "custom output"
        ));
    }

    #[test]
    fn token_usage_projects_to_conversation_notification() {
        let mut projector = ConversationNotificationProjector::new("default");
        let usage = agent_protocol::ModelUsage {
            input_tokens: 100,
            cached_input_tokens: 25,
            output_tokens: 40,
            reasoning_output_tokens: 5,
            total_tokens: 140,
        };

        let notifications = projector.project_turn_event(&EventMsg::TokenUsageUpdated {
            turn_id: "turn-1".to_string(),
            last_usage: usage.clone(),
            total_usage: usage.clone(),
            model_context_window: Some(1000),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::TokenUsageUpdated {
                conversation_id,
                turn_id,
                last_usage,
                total_usage,
                model_context_window,
            } if conversation_id == "default"
                && turn_id == "turn-1"
                && last_usage.total_tokens == 140
                && total_usage.cached_input_tokens == 25
                && *model_context_window == Some(1000)
        ));
    }

    #[test]
    fn delta_without_started_reports_lifecycle_error() {
        let mut projector = ConversationNotificationProjector::new("default");

        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:missing".to_string(),
            kind: TurnItemDeltaKind::Text,
            delta: "hello".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::Error { message, .. }
                if message.contains("before item start")
        ));
    }

    #[test]
    fn item_completed_clears_active_lifecycle() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            item: TranscriptItem::AgentMessage {
                id: "assistant:1".to_string(),
                text: "done".to_string(),
            },
        });
        let terminal = projector.project_turn_event(&EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        });
        let flushed = projector.finish_turn(TurnState::Completed);

        assert_eq!(completed.len(), 1);
        assert!(terminal.is_empty());
        assert_eq!(flushed.len(), 2);
        assert!(
            !flushed
                .iter()
                .any(|notification| matches!(notification, AppServerNotification::Error { .. }))
        );
    }

    #[test]
    fn turn_completed_reports_dangling_active_items() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        projector.project_turn_event(&EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        });
        let flushed = projector.finish_turn(TurnState::Completed);

        assert!(flushed.iter().any(|notification| matches!(
            notification,
            AppServerNotification::Error { message, .. }
                if message.contains("completed with active items")
        )));
    }
}
