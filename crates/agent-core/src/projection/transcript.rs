use crate::conversation::{ResponseItem, TranscriptItem};
use crate::rollout::RolloutItem;
use crate::tool::StructuredToolResult;
use crate::turn::EventMsg;
use std::collections::HashMap;

#[derive(Default)]
pub struct TranscriptBuilder {
    items: Vec<TranscriptItem>,
    positions: HashMap<String, usize>,
}

impl TranscriptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_rollout_item(&mut self, item: &RolloutItem) {
        match item {
            RolloutItem::EventMsg { event } => self.push_event_msg(event),
            RolloutItem::ResponseItem { item } => self.push_response_item(item),
            RolloutItem::Compacted { summary } => {
                self.push_unique(
                    TranscriptItem::SystemMessage {
                        id: "compacted".to_string(),
                        text: summary.clone(),
                    },
                    false,
                );
            }
            RolloutItem::TurnContext { .. } | RolloutItem::SessionMeta { .. } => {}
        }
    }

    pub fn finish(self) -> Vec<TranscriptItem> {
        self.items
    }

    fn push_response_item(&mut self, item: &ResponseItem) {
        if let Some(item) = transcript_item_from_response_item(item) {
            self.push_unique(item, false);
        }
    }

    fn push_event_msg(&mut self, event: &EventMsg) {
        match event {
            EventMsg::TurnStarted { user_input, .. } => {
                self.push_unique(
                    TranscriptItem::UserMessage {
                        id: String::new(),
                        text: user_input.clone(),
                    },
                    false,
                );
            }
            EventMsg::ItemCompleted { item, .. } => {
                self.push_unique(item.clone(), true);
            }
            EventMsg::TurnFailed { error, .. } => {
                self.push_unique(
                    TranscriptItem::SystemMessage {
                        id: "turn_failed".to_string(),
                        text: error.clone(),
                    },
                    false,
                );
            }
            EventMsg::TurnCancelled { reason, .. } => {
                self.push_unique(
                    TranscriptItem::SystemMessage {
                        id: "turn_cancelled".to_string(),
                        text: reason.clone(),
                    },
                    false,
                );
            }
            EventMsg::ModelRequestStarted { .. }
            | EventMsg::ModelResponseReceived { .. }
            | EventMsg::ItemStarted { .. }
            | EventMsg::ItemDelta { .. }
            | EventMsg::ServerRequestRequested { .. }
            | EventMsg::ServerRequestResolved { .. }
            | EventMsg::TurnCompleted { .. } => {}
        }
    }

    fn push_unique(&mut self, item: TranscriptItem, replace_existing: bool) {
        if transcript_item_is_empty(&item) {
            return;
        }
        let key = transcript_item_key(&item);
        if let Some(index) = self.positions.get(&key).copied() {
            if replace_existing {
                self.items[index] = item;
            }
            return;
        }
        self.positions.insert(key, self.items.len());
        self.items.push(item);
    }
}

pub fn transcript_items_from_rollout_items(items: &[RolloutItem]) -> Vec<TranscriptItem> {
    let mut builder = TranscriptBuilder::new();
    for item in items {
        builder.push_rollout_item(item);
    }
    builder.finish()
}

pub fn transcript_items_from_response_items(items: &[ResponseItem]) -> Vec<TranscriptItem> {
    let mut builder = TranscriptBuilder::new();
    for item in items {
        builder.push_response_item(item);
    }
    builder.finish()
}

pub fn transcript_item_from_response_item(message: &ResponseItem) -> Option<TranscriptItem> {
    match message {
        ResponseItem::System { content } => Some(TranscriptItem::SystemMessage {
            id: "system".to_string(),
            text: content.clone(),
        }),
        ResponseItem::User { content } => Some(TranscriptItem::UserMessage {
            id: String::new(),
            text: content.clone(),
        }),
        ResponseItem::Assistant { content, .. } => Some(TranscriptItem::AgentMessage {
            id: String::new(),
            text: content.clone().unwrap_or_default(),
        }),
        ResponseItem::Tool {
            tool_call_id,
            name,
            content,
            structured,
        } => Some(transcript_item_from_tool_response(
            tool_call_id,
            name,
            content,
            structured.as_ref(),
        )),
    }
}

fn transcript_item_from_tool_response(
    tool_call_id: &str,
    name: &str,
    content: &str,
    structured: Option<&StructuredToolResult>,
) -> TranscriptItem {
    match structured {
        Some(StructuredToolResult::CommandExecution {
            command,
            current_directory,
            status,
            exit_code,
            stdout,
            stderr,
            ..
        }) => TranscriptItem::CommandExecution {
            id: tool_call_id.to_string(),
            tool_name: name.to_string(),
            command: command.clone(),
            current_directory: current_directory.clone(),
            status: status.clone(),
            exit_code: *exit_code,
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            summary: content.to_string(),
        },
        Some(StructuredToolResult::WriteFile {
            path,
            bytes_written,
            status,
        }) => TranscriptItem::FileChange {
            id: tool_call_id.to_string(),
            tool_name: name.to_string(),
            path: path.clone(),
            status: status.clone(),
            bytes_written: *bytes_written,
            summary: content.to_string(),
        },
        structured => TranscriptItem::ToolResult {
            id: tool_call_id.to_string(),
            tool_name: name.to_string(),
            content: content.to_string(),
            summary: content.to_string(),
            structured: structured.cloned(),
        },
    }
}

fn transcript_item_key(item: &TranscriptItem) -> String {
    match item {
        TranscriptItem::SystemMessage { text, .. } => format!("system:{text}"),
        TranscriptItem::UserMessage { text, .. } => format!("user:{text}"),
        TranscriptItem::AgentMessage { text, .. } => format!("agent:{text}"),
        TranscriptItem::CommandExecution {
            tool_name,
            command,
            current_directory,
            ..
        } => format!("command:{tool_name}:{command}:{current_directory}"),
        TranscriptItem::FileChange {
            tool_name,
            path,
            summary,
            ..
        } => format!("file:{tool_name}:{path}:{summary}"),
        TranscriptItem::ToolResult {
            tool_name,
            content,
            summary,
            ..
        } => format!("tool:{tool_name}:{content}:{summary}"),
        TranscriptItem::Reasoning { title, text, .. } => format!("reasoning:{title}:{text}"),
    }
}

fn transcript_item_is_empty(item: &TranscriptItem) -> bool {
    match item {
        TranscriptItem::SystemMessage { text, .. }
        | TranscriptItem::UserMessage { text, .. }
        | TranscriptItem::AgentMessage { text, .. }
        | TranscriptItem::Reasoning { text, .. } => text.trim().is_empty(),
        TranscriptItem::CommandExecution { summary, .. }
        | TranscriptItem::FileChange { summary, .. }
        | TranscriptItem::ToolResult { summary, .. } => summary.trim().is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{CommandExecutionStatus, StructuredToolResult};
    use crate::turn::TurnItemKind;

    #[test]
    fn transcript_builder_projects_rollout_facts_without_duplicate_messages() {
        let assistant = TranscriptItem::AgentMessage {
            id: "assistant-1".to_string(),
            text: "hello".to_string(),
        };
        let items = vec![
            RolloutItem::from(ResponseItem::User {
                content: "hi".to_string(),
            }),
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-1".to_string(),
                conversation_id: "default".to_string(),
                user_input: "hi".to_string(),
            }),
            RolloutItem::from(EventMsg::ItemCompleted {
                turn_id: "turn-1".to_string(),
                item_id: "assistant-1".to_string(),
                item: assistant.clone(),
            }),
            RolloutItem::from(ResponseItem::Assistant {
                content: Some("hello".to_string()),
                tool_calls: Vec::new(),
            }),
            RolloutItem::from(EventMsg::TurnCompleted {
                turn_id: "turn-1".to_string(),
                final_response: "hello".to_string(),
            }),
        ];

        let transcript = transcript_items_from_rollout_items(&items);

        assert_eq!(transcript.len(), 2);
        assert!(matches!(transcript[0], TranscriptItem::UserMessage { .. }));
        assert!(matches!(
            &transcript[1],
            TranscriptItem::AgentMessage { text, .. } if text == "hello"
        ));
    }

    #[test]
    fn transcript_builder_keeps_rich_tool_projection() {
        let item = transcript_item_from_response_item(&ResponseItem::Tool {
            tool_call_id: "call-1".to_string(),
            name: "shell_command".to_string(),
            content: "D:\\learn\\gifti\\cloudagent".to_string(),
            structured: Some(StructuredToolResult::CommandExecution {
                command: "pwd".to_string(),
                current_directory: "D:\\learn\\gifti\\cloudagent".to_string(),
                status: CommandExecutionStatus::Completed,
                exit_code: Some(0),
                success: Some(true),
                stdout: Some("D:\\learn\\gifti\\cloudagent".to_string()),
                stderr: Some(String::new()),
            }),
        })
        .expect("tool response should project");

        assert!(matches!(
            item,
            TranscriptItem::CommandExecution {
                command,
                status: CommandExecutionStatus::Completed,
                ..
            } if command == "pwd"
        ));
    }

    #[test]
    fn lifecycle_only_events_do_not_create_transcript_items() {
        let items = vec![
            RolloutItem::from(EventMsg::ItemStarted {
                turn_id: "turn-1".to_string(),
                item_id: "item-1".to_string(),
                kind: TurnItemKind::AssistantMessage,
                title: None,
            }),
            RolloutItem::from(EventMsg::TurnCompleted {
                turn_id: "turn-1".to_string(),
                final_response: "done".to_string(),
            }),
            RolloutItem::from(EventMsg::TurnFailed {
                turn_id: "turn-2".to_string(),
                error: String::new(),
            }),
        ];

        assert!(transcript_items_from_rollout_items(&items).is_empty());
    }
}
