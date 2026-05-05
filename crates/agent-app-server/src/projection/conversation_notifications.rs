use agent_core::{CoreTranscriptEvent, core_transcript_event_from_event_msg};
use agent_protocol::{AppServerNotification, EventMsg, FrontendMode, TurnItemDeltaKind, TurnState};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct ActiveLifecycle {
    turn_id: String,
    item_id: String,
    call_id: Option<String>,
}

pub(crate) struct ConversationNotificationProjector {
    conversation_id: String,
    deferred_terminal_notifications: Vec<AppServerNotification>,
    active_items_by_item_id: HashMap<String, ActiveLifecycle>,
    active_item_id_by_call_id: HashMap<String, String>,
}

impl ConversationNotificationProjector {
    pub(crate) fn new(conversation_id: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            deferred_terminal_notifications: Vec::new(),
            active_items_by_item_id: HashMap::new(),
            active_item_id_by_call_id: HashMap::new(),
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
                call_id,
                kind,
                title,
            } => {
                let lifecycle = ActiveLifecycle {
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    call_id: call_id.clone(),
                };
                self.register_active_item(lifecycle);
                vec![AppServerNotification::ItemStarted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    call_id: call_id.clone(),
                    kind: kind.clone(),
                    title: title.clone(),
                }]
            }
            EventMsg::ItemDelta {
                turn_id,
                item_id,
                call_id,
                kind,
                delta,
            } => match kind {
                TurnItemDeltaKind::Text
                | TurnItemDeltaKind::ReasoningSummary
                | TurnItemDeltaKind::ReasoningText => Vec::new(),
                TurnItemDeltaKind::CommandExecutionOutput => {
                    if let Some(error) = self.validate_active_lifecycle(turn_id, item_id, call_id) {
                        return vec![error];
                    }
                    vec![AppServerNotification::CommandExecutionOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        call_id: call_id
                            .clone()
                            .or_else(|| self.call_id_for_item(item_id).map(str::to_owned)),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::ToolOutput => {
                    if let Some(error) = self.validate_active_lifecycle(turn_id, item_id, call_id) {
                        return vec![error];
                    }
                    vec![AppServerNotification::ToolOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        call_id: call_id
                            .clone()
                            .or_else(|| self.call_id_for_item(item_id).map(str::to_owned)),
                        delta: delta.clone(),
                    }]
                }
                TurnItemDeltaKind::FileChangeOutput => {
                    if let Some(error) = self.validate_active_lifecycle(turn_id, item_id, call_id) {
                        return vec![error];
                    }
                    vec![AppServerNotification::FileChangeOutputDelta {
                        conversation_id: self.conversation_id.clone(),
                        turn_id: turn_id.clone(),
                        item_id: item_id.clone(),
                        call_id: call_id
                            .clone()
                            .or_else(|| self.call_id_for_item(item_id).map(str::to_owned)),
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
                ..
            } => vec![AppServerNotification::TokenUsageUpdated {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
                last_usage: last_usage.clone(),
                total_usage: total_usage.clone(),
                model_context_window: *model_context_window,
            }],
            EventMsg::ModelRetrying {
                turn_id,
                stage,
                attempt,
                next_delay_ms,
            } => vec![AppServerNotification::ModelRetrying {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
                stage: stage.clone(),
                attempt: *attempt,
                next_delay_ms: *next_delay_ms,
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
            EventMsg::ContextCompactionStarted {
                turn_id,
                estimated_tokens,
            } => vec![AppServerNotification::ContextCompactionStarted {
                conversation_id: self.conversation_id.clone(),
                turn_id: turn_id.clone(),
                estimated_tokens: *estimated_tokens,
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
                self.retain_active_items_for_other_turns(&turn_id);
                self.deferred_terminal_notifications
                    .push(AppServerNotification::TurnCompleted {
                        conversation_id: self.conversation_id.clone(),
                        turn_id,
                    });
                Vec::new()
            }
            CoreTranscriptEvent::ItemCompleted {
                turn_id,
                call_id,
                item,
            } => {
                let item_id = item.id().to_string();
                let lifecycle_error =
                    self.validate_active_lifecycle(&turn_id, &item_id, &call_id);
                let lifecycle = self.remove_active_item(&item_id);
                let mut notifications = Vec::new();
                if let Some(error) = lifecycle_error {
                    notifications.push(error);
                }
                notifications.push(AppServerNotification::ItemCompleted {
                    conversation_id: self.conversation_id.clone(),
                    turn_id,
                    call_id: call_id.or_else(|| lifecycle.and_then(|it| it.call_id)),
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
        match self.active_items_by_item_id.get(item_id) {
            Some(active) if active.turn_id == turn_id => None,
            Some(active) => Some(self.lifecycle_error(format!(
                "item `{item_id}` belongs to turn `{}` but received event for turn `{turn_id}`",
                active.turn_id
            ))),
            None => Some(self.lifecycle_error(format!(
                "item `{item_id}` received lifecycle event before item start"
            ))),
        }
    }

    fn validate_active_call(&self, turn_id: &str, call_id: &str) -> Option<AppServerNotification> {
        let Some(item_id) = self.active_item_id_by_call_id.get(call_id) else {
            return Some(self.lifecycle_error(format!(
                "call `{call_id}` received lifecycle event before item start"
            )));
        };
        match self.active_items_by_item_id.get(item_id) {
            Some(active) if active.turn_id == turn_id => None,
            Some(active) => Some(self.lifecycle_error(format!(
                "call `{call_id}` belongs to turn `{}` but received event for turn `{turn_id}`",
                active.turn_id
            ))),
            None => Some(self.lifecycle_error(format!(
                "call `{call_id}` points to missing active item `{item_id}`"
            ))),
        }
    }

    fn validate_active_lifecycle(
        &self,
        turn_id: &str,
        item_id: &str,
        call_id: &Option<String>,
    ) -> Option<AppServerNotification> {
        if let Some(call_id) = call_id.as_deref() {
            if let Some(error) = self.validate_active_call(turn_id, call_id) {
                return Some(error);
            }
            if let Some(active) = self.active_items_by_item_id.get(item_id)
                && active.call_id.as_deref() != Some(call_id)
            {
                return Some(self.lifecycle_error(format!(
                    "item `{item_id}` is associated with call `{}` but received lifecycle event for call `{call_id}`",
                    active.call_id.as_deref().unwrap_or("<missing>")
                )));
            }
            return None;
        }
        self.validate_active_item(turn_id, item_id)
    }

    fn active_items_not_closed_error(&self, turn_id: &str) -> Option<AppServerNotification> {
        let dangling = self
            .active_items_by_item_id
            .iter()
            .filter(|(_, active)| active.turn_id == turn_id)
            .map(|(_, active)| active.item_id.as_str())
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

    fn register_active_item(&mut self, lifecycle: ActiveLifecycle) {
        if let Some(call_id) = lifecycle.call_id.as_ref() {
            self.active_item_id_by_call_id
                .insert(call_id.clone(), lifecycle.item_id.clone());
        }
        self.active_items_by_item_id
            .insert(lifecycle.item_id.clone(), lifecycle);
    }

    fn remove_active_item(&mut self, item_id: &str) -> Option<ActiveLifecycle> {
        let lifecycle = self.active_items_by_item_id.remove(item_id)?;
        if let Some(call_id) = lifecycle.call_id.as_ref() {
            self.active_item_id_by_call_id.remove(call_id);
        }
        Some(lifecycle)
    }

    fn call_id_for_item(&self, item_id: &str) -> Option<&str> {
        self.active_items_by_item_id
            .get(item_id)
            .and_then(|lifecycle| lifecycle.call_id.as_deref())
    }

    fn retain_active_items_for_other_turns(&mut self, turn_id: &str) {
        let stale_item_ids = self
            .active_items_by_item_id
            .iter()
            .filter(|(_, active)| active.turn_id == turn_id)
            .map(|(item_id, _)| item_id.clone())
            .collect::<Vec<_>>();
        for item_id in stale_item_ids {
            let _ = self.remove_active_item(&item_id);
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
            call_id: Some("call-write".to_string()),
            kind: TurnItemKind::FileChange,
            title: Some("edit_file".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:write".to_string(),
            call_id: Some("call-write".to_string()),
            kind: TurnItemDeltaKind::FileChangeOutput,
            delta: "wrote note.txt".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::FileChangeOutputDelta {
                item_id,
                call_id,
                delta,
                ..
            }
                if item_id == "tool:write"
                    && call_id.as_deref() == Some("call-write")
                    && delta == "wrote note.txt"
        ));
    }

    #[test]
    fn command_execution_output_delta_projects_to_command_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemKind::CommandExecution,
            title: Some("exec_command".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            delta: "D:\\work".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::CommandExecutionOutputDelta {
                item_id,
                call_id,
                delta,
                ..
            }
                if item_id == "tool:shell"
                    && call_id.as_deref() == Some("call-shell")
                    && delta == "D:\\work"
        ));
    }

    #[test]
    fn generic_tool_output_delta_projects_to_tool_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:custom".to_string(),
            call_id: Some("call-custom".to_string()),
            kind: TurnItemKind::ToolCall,
            title: Some("custom_tool".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:custom".to_string(),
            call_id: Some("call-custom".to_string()),
            kind: TurnItemDeltaKind::ToolOutput,
            delta: "custom output".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::ToolOutputDelta {
                item_id,
                call_id,
                delta,
                ..
            }
                if item_id == "tool:custom"
                    && call_id.as_deref() == Some("call-custom")
                    && delta == "custom output"
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
            request_estimated_tokens: 130,
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
    fn model_retrying_projects_to_conversation_notification() {
        let mut projector = ConversationNotificationProjector::new("default");

        let notifications = projector.project_turn_event(&EventMsg::ModelRetrying {
            turn_id: "turn-1".to_string(),
            stage: agent_protocol::ModelRetryStage::Streaming,
            attempt: 2,
            next_delay_ms: 500,
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::ModelRetrying {
                conversation_id,
                turn_id,
                stage,
                attempt,
                next_delay_ms,
            } if conversation_id == "default"
                && turn_id == "turn-1"
                && *stage == agent_protocol::ModelRetryStage::Streaming
                && *attempt == 2
                && *next_delay_ms == 500
        ));
    }

    #[test]
    fn delta_without_started_reports_lifecycle_error() {
        let mut projector = ConversationNotificationProjector::new("default");

        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:missing".to_string(),
            call_id: None,
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
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
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
    fn item_completed_notification_includes_call_id_field_for_compat() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        });
        let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            item: TranscriptItem::AgentMessage {
                id: "assistant:1".to_string(),
                text: "done".to_string(),
            },
        });

        assert!(matches!(
            &completed[0],
            AppServerNotification::ItemCompleted { call_id, .. } if call_id.is_none()
        ));
    }

    #[test]
    fn mismatched_call_id_reports_lifecycle_error() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemKind::CommandExecution,
            title: Some("exec_command".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-other".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            delta: "D:\\work".to_string(),
        });

        assert_eq!(notifications.len(), 1);
        assert!(matches!(
            &notifications[0],
            AppServerNotification::Error { message, .. }
                if message.contains("call `call-other` received lifecycle event before item start")
        ));
    }

    #[test]
    fn item_completed_prefers_event_call_id_when_lifecycle_is_missing() {
        let mut projector = ConversationNotificationProjector::new("default");

        let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            item: TranscriptItem::ToolResult {
                id: "tool:shell".to_string(),
                tool_name: "exec_command".to_string(),
                content: "done".to_string(),
                summary: "done".to_string(),
                structured: None,
            },
        });

        assert!(matches!(
            completed.last(),
            Some(AppServerNotification::ItemCompleted { call_id, .. })
                if call_id.as_deref() == Some("call-shell")
        ));
    }

    #[test]
    fn completed_item_removes_call_id_index() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemKind::CommandExecution,
            title: Some("exec_command".to_string()),
        });
        let _ = projector.project_turn_event(&EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            item: TranscriptItem::CommandExecution {
                id: "tool:shell".to_string(),
                tool_name: "exec_command".to_string(),
                command: "pwd".to_string(),
                current_directory: "D:\\work".to_string(),
                status: agent_protocol::CommandExecutionStatus::Completed,
                exit_code: Some(0),
                stdout: Some("D:\\work".to_string()),
                stderr: None,
                aggregated_output: Some("D:\\work".to_string()),
                duration_ms: Some(1),
                summary: "D:\\work".to_string(),
            },
        });

        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool:other".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            delta: "should fail".to_string(),
        });

        assert!(matches!(
            notifications.first(),
            Some(AppServerNotification::Error { message, .. })
                if message.contains("call `call-shell` received lifecycle event before item start")
        ));
    }

    #[test]
    fn call_id_turn_mismatch_reports_lifecycle_error() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemKind::CommandExecution,
            title: Some("exec_command".to_string()),
        });
        let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
            turn_id: "turn-2".to_string(),
            item_id: "tool:shell".to_string(),
            call_id: Some("call-shell".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            delta: "D:\\other".to_string(),
        });

        assert!(matches!(
            notifications.first(),
            Some(AppServerNotification::Error { message, .. })
                if message.contains("call `call-shell` belongs to turn `turn-1`")
        ));
    }

    #[test]
    fn turn_completed_reports_dangling_active_items() {
        let mut projector = ConversationNotificationProjector::new("default");

        projector.project_turn_event(&EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
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
