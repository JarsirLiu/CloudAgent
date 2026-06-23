use crate::conversation::{ConversationHistory, TranscriptItem};
use crate::tool::{CommandExecutionStatus, StructuredToolResult, ToolEvent, WriteFileStatus};
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
            EventMsg::ItemCompleted {
                transcript_item: item,
                ..
            } => match item {
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
                    tool_name,
                    summary,
                    structured,
                    ..
                } => {
                    tool_events.push(ToolEvent {
                        name: tool_name.clone(),
                        summary: summary.clone(),
                        is_error: structured
                            .as_ref()
                            .is_some_and(StructuredToolResult::is_error),
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
            | EventMsg::ModelRetrying { .. }
            | EventMsg::TokenUsageUpdated { .. }
            | EventMsg::ContextCompacted { .. }
            | EventMsg::ContextCompactionStarted { .. }
            | EventMsg::ItemStarted { .. }
            | EventMsg::ItemDelta { .. }
            | EventMsg::ItemProgress { .. }
            | EventMsg::ItemMetricsUpdated { .. }
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
#[path = "turn_output_tests.rs"]
mod tests;
