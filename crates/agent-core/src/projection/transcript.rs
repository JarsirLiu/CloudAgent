use crate::conversation::{ConversationTurn, ResponseItem, TranscriptItem};
use crate::rollout::RolloutItem;
use crate::tool::StructuredToolResult;
use crate::turn::{EventMsg, TurnState};
use std::collections::HashMap;

#[derive(Default)]
pub struct ConversationHistoryBuilder {
    turns: Vec<ConversationTurn>,
    current_turn: Option<PendingConversationTurn>,
    current_rollout_index: usize,
    next_rollout_index: usize,
}

struct PendingConversationTurn {
    id: String,
    state: TurnState,
    items: Vec<TranscriptItem>,
    positions: HashMap<String, usize>,
    rollout_start_index: usize,
    rollout_end_index: usize,
    opened_explicitly: bool,
}

impl ConversationHistoryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_rollout_item(&mut self, item: &RolloutItem) {
        self.current_rollout_index = self.next_rollout_index;
        self.next_rollout_index += 1;
        match item {
            RolloutItem::EventMsg { event } => self.push_event_msg(event),
            RolloutItem::ResponseItem { item } => self.push_response_item(item),
            RolloutItem::Compacted { summary } => {
                self.upsert_item_in_current_turn(TranscriptItem::SystemMessage {
                    id: format!("compacted:{}", self.current_rollout_index),
                    text: summary.clone(),
                });
            }
        }
    }

    pub fn finish(mut self) -> Vec<ConversationTurn> {
        self.finish_current_turn();
        self.turns
    }

    pub fn active_turn_snapshot(&self) -> Option<ConversationTurn> {
        self.current_turn
            .as_ref()
            .map(ConversationTurn::from)
            .or_else(|| self.turns.last().cloned())
    }

    fn new_turn(&self, id: String, opened_explicitly: bool) -> PendingConversationTurn {
        PendingConversationTurn {
            id,
            state: TurnState::Running,
            items: Vec::new(),
            positions: HashMap::new(),
            rollout_start_index: self.current_rollout_index,
            rollout_end_index: self.current_rollout_index,
            opened_explicitly,
        }
    }

    fn ensure_turn(&mut self) -> &mut PendingConversationTurn {
        if self.current_turn.is_none() {
            let id = format!("turn-{}", self.turns.len() + 1);
            self.current_turn = Some(self.new_turn(id, false));
        }
        self.current_turn
            .as_mut()
            .expect("ensure_turn must create current turn")
    }

    fn finish_current_turn(&mut self) {
        let Some(turn) = self.current_turn.take() else {
            return;
        };
        if turn.items.is_empty() && turn.state == TurnState::Running {
            return;
        }
        self.turns.push(ConversationTurn::from(&turn));
    }

    fn push_response_item(&mut self, item: &ResponseItem) {
        if self
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.opened_explicitly)
        {
            return;
        }
        if let Some(mut item) = transcript_item_from_response_item(item) {
            assign_response_item_id(&mut item, self.current_rollout_index);
            self.upsert_item_in_current_turn(item);
        }
    }

    fn push_event_msg(&mut self, event: &EventMsg) {
        match event {
            EventMsg::TurnStarted {
                turn_id,
                user_input,
                ..
            } => {
                if self
                    .current_turn
                    .as_ref()
                    .is_some_and(|turn| turn.opened_explicitly)
                {
                    self.finish_current_turn();
                } else {
                    self.current_turn = None;
                }
                self.current_turn = Some(self.new_turn(turn_id.clone(), true));
                self.upsert_item_in_current_turn(TranscriptItem::UserMessage {
                    id: format!("user:{turn_id}"),
                    text: user_input.clone(),
                });
            }
            EventMsg::ItemCompleted { turn_id, item, .. } => {
                self.upsert_item_in_turn_id(turn_id, item.clone());
            }
            EventMsg::TurnFailed { turn_id, error } => {
                self.upsert_item_in_turn_id(
                    turn_id,
                    TranscriptItem::SystemMessage {
                        id: format!("turn_failed:{turn_id}"),
                        text: error.clone(),
                    },
                );
                self.set_turn_state(turn_id, TurnState::Failed, true);
            }
            EventMsg::TurnCancelled { turn_id, reason } => {
                self.upsert_item_in_turn_id(
                    turn_id,
                    TranscriptItem::SystemMessage {
                        id: format!("turn_cancelled:{turn_id}"),
                        text: reason.clone(),
                    },
                );
                self.set_turn_state(turn_id, TurnState::Cancelled, true);
            }
            EventMsg::TurnCompleted { turn_id } => {
                self.set_turn_state(turn_id, TurnState::Completed, true);
            }
            EventMsg::ModelRequestStarted { .. }
            | EventMsg::ModelResponseReceived { .. }
            | EventMsg::ItemStarted { .. }
            | EventMsg::ItemDelta { .. }
            | EventMsg::ServerRequestRequested { .. }
            | EventMsg::ServerRequestResolved { .. } => {}
        }
    }

    fn set_turn_state(&mut self, turn_id: &str, state: TurnState, finish_if_current: bool) {
        if self
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.id == turn_id)
        {
            if let Some(turn) = self.current_turn.as_mut() {
                turn.state = state;
            }
            if finish_if_current {
                self.finish_current_turn();
            }
            return;
        }

        if let Some(turn) = self.turns.iter_mut().find(|turn| turn.id == turn_id) {
            turn.state = state;
        }
    }

    fn upsert_item_in_current_turn(&mut self, item: TranscriptItem) {
        if transcript_item_is_empty(&item) {
            return;
        }
        let current_rollout_index = self.current_rollout_index;
        let turn = self.ensure_turn();
        turn.upsert_item(item, current_rollout_index);
    }

    fn upsert_item_in_turn_id(&mut self, turn_id: &str, item: TranscriptItem) {
        if transcript_item_is_empty(&item) {
            return;
        }
        if self
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.id == turn_id)
        {
            let current_rollout_index = self.current_rollout_index;
            if let Some(turn) = self.current_turn.as_mut() {
                turn.upsert_item(item, current_rollout_index);
            }
            return;
        }

        if let Some(turn) = self.turns.iter_mut().find(|turn| turn.id == turn_id) {
            upsert_completed_turn_item(turn, item, self.current_rollout_index);
        }
    }
}

impl PendingConversationTurn {
    fn upsert_item(&mut self, item: TranscriptItem, rollout_index: usize) {
        self.rollout_end_index = rollout_index;
        let id = item.id().to_string();
        if let Some(index) = self.positions.get(&id).copied() {
            self.items[index] = item;
            return;
        }
        self.positions.insert(id, self.items.len());
        self.items.push(item);
    }
}

fn upsert_completed_turn_item(
    turn: &mut ConversationTurn,
    item: TranscriptItem,
    rollout_index: usize,
) {
    if let Some(existing) = turn
        .items
        .iter_mut()
        .find(|existing| existing.id() == item.id())
    {
        *existing = item;
    } else {
        turn.items.push(item);
    }
    turn.rollout_end_index = rollout_index;
}

impl From<&PendingConversationTurn> for ConversationTurn {
    fn from(turn: &PendingConversationTurn) -> Self {
        Self {
            id: turn.id.clone(),
            state: turn.state.clone(),
            items: turn.items.clone(),
            rollout_start_index: turn.rollout_start_index,
            rollout_end_index: turn.rollout_end_index,
        }
    }
}

#[derive(Default)]
pub struct TranscriptBuilder {
    history: ConversationHistoryBuilder,
}

impl TranscriptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_rollout_item(&mut self, item: &RolloutItem) {
        self.history.push_rollout_item(item);
    }

    pub fn finish(self) -> Vec<TranscriptItem> {
        flatten_conversation_turns(&self.history.finish())
    }
}

pub fn build_turns_from_rollout_items(items: &[RolloutItem]) -> Vec<ConversationTurn> {
    let mut builder = ConversationHistoryBuilder::new();
    for item in items {
        builder.push_rollout_item(item);
    }
    builder.finish()
}

pub fn transcript_items_from_rollout_items(items: &[RolloutItem]) -> Vec<TranscriptItem> {
    flatten_conversation_turns(&build_turns_from_rollout_items(items))
}

pub fn transcript_items_from_response_items(items: &[ResponseItem]) -> Vec<TranscriptItem> {
    let mut builder = ConversationHistoryBuilder::new();
    for (index, item) in items.iter().enumerate() {
        builder.current_rollout_index = index;
        builder.push_response_item(item);
    }
    flatten_conversation_turns(&builder.finish())
}

pub fn flatten_conversation_turns(turns: &[ConversationTurn]) -> Vec<TranscriptItem> {
    turns
        .iter()
        .flat_map(|turn| turn.items.iter().cloned())
        .collect()
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

fn assign_response_item_id(item: &mut TranscriptItem, rollout_index: usize) {
    let id = match item {
        TranscriptItem::SystemMessage { id, .. }
        | TranscriptItem::UserMessage { id, .. }
        | TranscriptItem::AgentMessage { id, .. }
        | TranscriptItem::CommandExecution { id, .. }
        | TranscriptItem::FileChange { id, .. }
        | TranscriptItem::ToolResult { id, .. }
        | TranscriptItem::Reasoning { id, .. } => id,
    };
    if id.is_empty() {
        *id = format!("response:{rollout_index}");
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
            }),
            RolloutItem::from(EventMsg::TurnFailed {
                turn_id: "turn-2".to_string(),
                error: String::new(),
            }),
        ];

        assert!(transcript_items_from_rollout_items(&items).is_empty());
    }

    #[test]
    fn conversation_history_builder_preserves_turn_boundaries() {
        let items = vec![
            RolloutItem::from(ResponseItem::User {
                content: "first".to_string(),
            }),
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-1".to_string(),
                conversation_id: "default".to_string(),
                user_input: "first".to_string(),
            }),
            RolloutItem::from(EventMsg::ItemCompleted {
                turn_id: "turn-1".to_string(),
                item_id: "assistant-1".to_string(),
                item: TranscriptItem::AgentMessage {
                    id: "assistant-1".to_string(),
                    text: "one".to_string(),
                },
            }),
            RolloutItem::from(EventMsg::TurnCompleted {
                turn_id: "turn-1".to_string(),
            }),
            RolloutItem::from(ResponseItem::User {
                content: "second".to_string(),
            }),
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-2".to_string(),
                conversation_id: "default".to_string(),
                user_input: "second".to_string(),
            }),
            RolloutItem::from(EventMsg::ItemCompleted {
                turn_id: "turn-2".to_string(),
                item_id: "assistant-2".to_string(),
                item: TranscriptItem::AgentMessage {
                    id: "assistant-2".to_string(),
                    text: "two".to_string(),
                },
            }),
            RolloutItem::from(EventMsg::TurnCompleted {
                turn_id: "turn-2".to_string(),
            }),
        ];

        let turns = build_turns_from_rollout_items(&items);

        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].id, "turn-1");
        assert_eq!(turns[0].state, TurnState::Completed);
        assert_eq!(turns[1].id, "turn-2");
        assert_eq!(turns[1].state, TurnState::Completed);
        assert!(matches!(
            &turns[0].items[..],
            [
                TranscriptItem::UserMessage { text: first, .. },
                TranscriptItem::AgentMessage { text: one, .. }
            ] if first == "first" && one == "one"
        ));
        assert!(matches!(
            &turns[1].items[..],
            [
                TranscriptItem::UserMessage { text: second, .. },
                TranscriptItem::AgentMessage { text: two, .. }
            ] if second == "second" && two == "two"
        ));
    }

    #[test]
    fn same_assistant_text_keeps_distinct_items_by_id() {
        let items = vec![
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-1".to_string(),
                conversation_id: "default".to_string(),
                user_input: "repeat".to_string(),
            }),
            RolloutItem::from(EventMsg::ItemCompleted {
                turn_id: "turn-1".to_string(),
                item_id: "assistant-1".to_string(),
                item: TranscriptItem::AgentMessage {
                    id: "assistant-1".to_string(),
                    text: "same".to_string(),
                },
            }),
            RolloutItem::from(EventMsg::ItemCompleted {
                turn_id: "turn-1".to_string(),
                item_id: "assistant-2".to_string(),
                item: TranscriptItem::AgentMessage {
                    id: "assistant-2".to_string(),
                    text: "same".to_string(),
                },
            }),
            RolloutItem::from(EventMsg::TurnCompleted {
                turn_id: "turn-1".to_string(),
            }),
        ];

        let turns = build_turns_from_rollout_items(&items);

        assert_eq!(turns.len(), 1);
        assert_eq!(
            turns[0]
                .items
                .iter()
                .filter(|item| matches!(item, TranscriptItem::AgentMessage { text, .. } if text == "same"))
                .count(),
            2
        );
    }

    #[test]
    fn late_item_completed_updates_original_turn_by_turn_id() {
        let items = vec![
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-1".to_string(),
                conversation_id: "default".to_string(),
                user_input: "first".to_string(),
            }),
            RolloutItem::from(EventMsg::TurnCompleted {
                turn_id: "turn-1".to_string(),
            }),
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-2".to_string(),
                conversation_id: "default".to_string(),
                user_input: "second".to_string(),
            }),
            RolloutItem::from(EventMsg::ItemCompleted {
                turn_id: "turn-1".to_string(),
                item_id: "assistant-late".to_string(),
                item: TranscriptItem::AgentMessage {
                    id: "assistant-late".to_string(),
                    text: "late answer".to_string(),
                },
            }),
            RolloutItem::from(EventMsg::TurnCompleted {
                turn_id: "turn-2".to_string(),
            }),
        ];

        let turns = build_turns_from_rollout_items(&items);

        assert_eq!(turns.len(), 2);
        assert!(matches!(
            &turns[0].items[..],
            [
                TranscriptItem::UserMessage { text: first, .. },
                TranscriptItem::AgentMessage { text: answer, .. }
            ] if first == "first" && answer == "late answer"
        ));
        assert!(matches!(
            &turns[1].items[..],
            [TranscriptItem::UserMessage { text, .. }] if text == "second"
        ));
    }

    #[test]
    fn same_tool_summary_keeps_distinct_items_by_id() {
        let command_item = |id: &str| TranscriptItem::CommandExecution {
            id: id.to_string(),
            tool_name: "shell_command".to_string(),
            command: "pwd".to_string(),
            current_directory: "D:\\work".to_string(),
            status: CommandExecutionStatus::Completed,
            exit_code: Some(0),
            stdout: Some("D:\\work".to_string()),
            stderr: Some(String::new()),
            summary: "D:\\work".to_string(),
        };
        let items = vec![
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-1".to_string(),
                conversation_id: "default".to_string(),
                user_input: "twice".to_string(),
            }),
            RolloutItem::from(EventMsg::ItemCompleted {
                turn_id: "turn-1".to_string(),
                item_id: "tool-1".to_string(),
                item: command_item("tool-1"),
            }),
            RolloutItem::from(EventMsg::ItemCompleted {
                turn_id: "turn-1".to_string(),
                item_id: "tool-2".to_string(),
                item: command_item("tool-2"),
            }),
            RolloutItem::from(EventMsg::TurnCompleted {
                turn_id: "turn-1".to_string(),
            }),
        ];

        let turns = build_turns_from_rollout_items(&items);

        assert_eq!(turns.len(), 1);
        assert_eq!(
            turns[0]
                .items
                .iter()
                .filter(|item| matches!(item, TranscriptItem::CommandExecution { summary, .. } if summary == "D:\\work"))
                .count(),
            2
        );
    }
}
