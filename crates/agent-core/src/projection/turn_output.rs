use crate::conversation::{ConversationHistory, TranscriptItem};
use crate::tool::{CommandExecutionStatus, ToolEvent, WriteFileStatus};
use crate::turn::{AgentTurnOutput, EventMsg, TurnState};

pub fn agent_turn_output_from_events(
    turn_id: String,
    events: Vec<EventMsg>,
    history: &ConversationHistory,
    model_name: Option<String>,
    state: TurnState,
) -> AgentTurnOutput {
    let tool_events = tool_events_from_turn_events(&events);
    AgentTurnOutput {
        turn_id,
        tool_events,
        events,
        model_name,
        total_messages: history.messages.len(),
        state,
    }
}

pub fn tool_events_from_turn_events(events: &[EventMsg]) -> Vec<ToolEvent> {
    let mut tool_events = Vec::new();

    for event in events {
        match event {
            EventMsg::ItemCompleted { item, .. } => match item {
                TranscriptItem::CommandExecution {
                    tool_name,
                    status,
                    summary,
                    ..
                } => {
                    tool_events.push(ToolEvent {
                        name: tool_name.clone(),
                        summary: summary.clone(),
                        is_error: *status != CommandExecutionStatus::Completed,
                    });
                }
                TranscriptItem::ToolResult {
                    tool_name, summary, ..
                } => {
                    let lower = summary.to_lowercase();
                    let is_error = lower.contains("error")
                        || lower.contains("failed")
                        || lower.contains("denied")
                        || lower.contains("skipped");
                    tool_events.push(ToolEvent {
                        name: tool_name.clone(),
                        summary: summary.clone(),
                        is_error,
                    });
                }
                TranscriptItem::FileChange {
                    tool_name,
                    status,
                    summary,
                    ..
                } => {
                    tool_events.push(ToolEvent {
                        name: tool_name.clone(),
                        summary: summary.clone(),
                        is_error: *status != WriteFileStatus::Completed,
                    });
                }
                TranscriptItem::UserMessage { .. }
                | TranscriptItem::SystemMessage { .. }
                | TranscriptItem::AgentMessage { .. }
                | TranscriptItem::Reasoning { .. } => {}
            },
            EventMsg::TurnStarted { .. }
            | EventMsg::ModelRequestStarted { .. }
            | EventMsg::ModelResponseReceived { .. }
            | EventMsg::ItemStarted { .. }
            | EventMsg::ItemDelta { .. }
            | EventMsg::ServerRequestRequested { .. }
            | EventMsg::ServerRequestResolved { .. }
            | EventMsg::TurnCompleted { .. }
            | EventMsg::TurnFailed { .. }
            | EventMsg::TurnCancelled { .. } => {}
        }
    }

    tool_events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::TurnItemDeltaKind;

    #[test]
    fn tool_event_uses_completed_item_not_streamed_fallback() {
        let events = vec![
            EventMsg::ItemStarted {
                turn_id: "turn-1".to_string(),
                item_id: "tool-1".to_string(),
                kind: crate::turn::TurnItemKind::CommandExecution,
                title: Some("fallback title".to_string()),
            },
            EventMsg::ItemDelta {
                turn_id: "turn-1".to_string(),
                item_id: "tool-1".to_string(),
                kind: TurnItemDeltaKind::CommandExecutionOutput,
                delta: "streamed fallback".to_string(),
            },
            EventMsg::ItemCompleted {
                turn_id: "turn-1".to_string(),
                item_id: "tool-1".to_string(),
                item: TranscriptItem::CommandExecution {
                    id: "tool-1".to_string(),
                    tool_name: "shell_command".to_string(),
                    command: "pwd".to_string(),
                    current_directory: "D:\\work".to_string(),
                    status: CommandExecutionStatus::Completed,
                    exit_code: Some(0),
                    stdout: Some("D:\\work".to_string()),
                    stderr: Some(String::new()),
                    summary: "completed summary".to_string(),
                },
            },
        ];

        let tool_events = tool_events_from_turn_events(&events);

        assert_eq!(tool_events.len(), 1);
        assert_eq!(tool_events[0].name, "shell_command");
        assert_eq!(tool_events[0].summary, "completed summary");
        assert!(!tool_events[0].is_error);
    }

    #[test]
    fn completed_tool_item_is_authoritative_without_started_event() {
        let events = vec![EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            item: TranscriptItem::ToolResult {
                id: "tool-1".to_string(),
                tool_name: "custom_tool".to_string(),
                content: "ok".to_string(),
                summary: "ok".to_string(),
                structured: None,
            },
        }];

        let tool_events = tool_events_from_turn_events(&events);

        assert_eq!(tool_events.len(), 1);
        assert_eq!(tool_events[0].name, "custom_tool");
        assert_eq!(tool_events[0].summary, "ok");
        assert!(!tool_events[0].is_error);
    }
}
