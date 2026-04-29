use crate::conversation::{ConversationHistory, TranscriptItem};
use crate::tool::{CommandExecutionStatus, ToolEvent, WriteFileStatus};
use crate::turn::{AgentTurnOutput, EventMsg, TurnItemKind, TurnState};
use std::collections::HashMap;

pub fn agent_turn_output_from_events(
    turn_id: String,
    final_response: String,
    events: Vec<EventMsg>,
    history: &ConversationHistory,
    model_name: Option<String>,
    state: TurnState,
) -> AgentTurnOutput {
    let tool_events = tool_events_from_turn_events(&events);
    AgentTurnOutput {
        turn_id,
        final_response,
        tool_events,
        events,
        model_name,
        total_messages: history.messages.len(),
        state,
    }
}

pub fn tool_events_from_turn_events(events: &[EventMsg]) -> Vec<ToolEvent> {
    let mut active_tools: HashMap<String, (String, String)> = HashMap::new();
    let mut tool_events = Vec::new();

    for event in events {
        match event {
            EventMsg::ItemStarted {
                item_id,
                kind,
                title,
                ..
            } if *kind == TurnItemKind::ToolCall
                || *kind == TurnItemKind::CommandExecution
                || *kind == TurnItemKind::FileChange =>
            {
                active_tools.insert(
                    item_id.clone(),
                    (
                        title.clone().unwrap_or_else(|| "tool_call".to_string()),
                        String::new(),
                    ),
                );
            }
            EventMsg::ItemDelta { item_id, delta, .. } => {
                if let Some((_, summary)) = active_tools.get_mut(item_id) {
                    if !summary.is_empty() {
                        summary.push('\n');
                    }
                    summary.push_str(delta);
                }
            }
            EventMsg::ItemCompleted { item, .. } => match item {
                TranscriptItem::CommandExecution {
                    id,
                    tool_name,
                    status,
                    summary,
                    ..
                } => {
                    if let Some((fallback_name, streamed_summary)) = active_tools.remove(id) {
                        let final_summary = if summary.trim().is_empty() {
                            streamed_summary
                        } else {
                            summary.clone()
                        };
                        let name = if tool_name.is_empty() {
                            fallback_name
                        } else {
                            tool_name.clone()
                        };
                        tool_events.push(ToolEvent {
                            name,
                            summary: final_summary,
                            is_error: *status != CommandExecutionStatus::Completed,
                        });
                    }
                }
                TranscriptItem::ToolResult {
                    id,
                    tool_name,
                    summary,
                    ..
                } => {
                    if let Some((fallback_name, streamed_summary)) = active_tools.remove(id) {
                        let final_summary = if summary.trim().is_empty() {
                            streamed_summary
                        } else {
                            summary.clone()
                        };
                        let name = if tool_name.is_empty() {
                            fallback_name
                        } else {
                            tool_name.clone()
                        };
                        let lower = final_summary.to_lowercase();
                        let is_error = lower.contains("error")
                            || lower.contains("failed")
                            || lower.contains("denied")
                            || lower.contains("skipped");
                        tool_events.push(ToolEvent {
                            name,
                            summary: final_summary,
                            is_error,
                        });
                    }
                }
                TranscriptItem::FileChange {
                    id,
                    tool_name,
                    status,
                    summary,
                    ..
                } => {
                    if let Some((fallback_name, streamed_summary)) = active_tools.remove(id) {
                        let final_summary = if summary.trim().is_empty() {
                            streamed_summary
                        } else {
                            summary.clone()
                        };
                        let name = if tool_name.is_empty() {
                            fallback_name
                        } else {
                            tool_name.clone()
                        };
                        tool_events.push(ToolEvent {
                            name,
                            summary: final_summary,
                            is_error: *status != WriteFileStatus::Completed,
                        });
                    }
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
            | EventMsg::ServerRequestRequested { .. }
            | EventMsg::ServerRequestResolved { .. }
            | EventMsg::TurnCompleted { .. }
            | EventMsg::TurnFailed { .. }
            | EventMsg::TurnCancelled { .. } => {}
        }
    }

    tool_events
}
