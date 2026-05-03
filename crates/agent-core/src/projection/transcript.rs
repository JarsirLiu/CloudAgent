use crate::conversation::{ConversationTurn, ResponseItem, TranscriptItem};
use crate::rollout::RolloutItem;
use crate::tool::StructuredToolResult;
use crate::turn::{EventMsg, TurnItemDeltaKind, TurnItemKind, TurnState};
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
            RolloutItem::Compacted {
                rendered_summary, ..
            } => {
                self.upsert_item_in_current_turn(TranscriptItem::SystemMessage {
                    id: format!("compacted:{}", self.current_rollout_index),
                    text: rendered_summary.clone(),
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
            EventMsg::ItemStarted {
                turn_id,
                item_id,
                kind,
                title,
            } => {
                if let Some(item) = transcript_item_from_item_start(item_id, kind, title.as_deref())
                {
                    self.upsert_item_in_turn_id_allow_empty(turn_id, item);
                }
            }
            EventMsg::ItemDelta {
                turn_id,
                item_id,
                kind,
                delta,
            } => {
                self.append_delta_to_item(turn_id, item_id, kind, delta);
            }
            EventMsg::ItemCompleted { turn_id, item, .. } => {
                self.upsert_item_in_turn_id(turn_id, item.clone());
            }
            EventMsg::TurnFailed { turn_id, error } => {
                self.mark_unfinished_items_in_turn(turn_id, "failed");
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
                self.mark_unfinished_items_in_turn(turn_id, "aborted");
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
            | EventMsg::TokenUsageUpdated { .. }
            | EventMsg::ContextCompacted { .. }
            | EventMsg::ContextCompactionStarted { .. }
            | EventMsg::ServerRequestRequested { .. }
            | EventMsg::ServerRequestResolved { .. } => {}
        }
    }

    fn append_delta_to_item(
        &mut self,
        turn_id: &str,
        item_id: &str,
        kind: &TurnItemDeltaKind,
        delta: &str,
    ) {
        if delta.is_empty() {
            return;
        }
        if self
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.id == turn_id)
        {
            let current_rollout_index = self.current_rollout_index;
            if let Some(turn) = self.current_turn.as_mut() {
                turn.append_delta(item_id, kind, delta, current_rollout_index);
            }
            return;
        }

        if let Some(turn) = self.turns.iter_mut().find(|turn| turn.id == turn_id) {
            append_delta_to_completed_turn(turn, item_id, kind, delta, self.current_rollout_index);
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
        self.upsert_item_in_turn_id_allow_empty(turn_id, item);
    }

    fn upsert_item_in_turn_id_allow_empty(&mut self, turn_id: &str, item: TranscriptItem) {
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

    fn mark_unfinished_items_in_turn(&mut self, turn_id: &str, reason: &str) {
        if self
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.id == turn_id)
        {
            if let Some(turn) = self.current_turn.as_mut() {
                turn.mark_unfinished_items(reason);
            }
            return;
        }

        if let Some(turn) = self.turns.iter_mut().find(|turn| turn.id == turn_id) {
            mark_unfinished_transcript_items(&mut turn.items, reason);
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

    fn append_delta(
        &mut self,
        item_id: &str,
        kind: &TurnItemDeltaKind,
        delta: &str,
        rollout_index: usize,
    ) {
        let Some(index) = self.positions.get(item_id).copied() else {
            return;
        };
        append_delta_to_transcript_item(&mut self.items[index], kind, delta);
        self.rollout_end_index = rollout_index;
    }

    fn mark_unfinished_items(&mut self, reason: &str) {
        mark_unfinished_transcript_items(&mut self.items, reason);
    }
}

fn mark_unfinished_transcript_items(items: &mut [TranscriptItem], reason: &str) {
    for item in items {
        match item {
            TranscriptItem::CommandExecution {
                status,
                stderr,
                aggregated_output,
                summary,
                ..
            } if *status == crate::tool::CommandExecutionStatus::InProgress => {
                *status = crate::tool::CommandExecutionStatus::Failed;
                *stderr = Some(reason.to_string());
                *aggregated_output = Some(reason.to_string());
                *summary = reason.to_string();
            }
            TranscriptItem::FileChange {
                status, summary, ..
            } if *status == crate::tool::WriteFileStatus::InProgress => {
                *status = crate::tool::WriteFileStatus::Failed;
                *summary = reason.to_string();
            }
            TranscriptItem::ToolResult {
                content, summary, ..
            } if content.trim().is_empty() && summary.trim().is_empty() => {
                *content = reason.to_string();
                *summary = reason.to_string();
            }
            _ => {}
        }
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

fn append_delta_to_completed_turn(
    turn: &mut ConversationTurn,
    item_id: &str,
    kind: &TurnItemDeltaKind,
    delta: &str,
    rollout_index: usize,
) {
    let Some(item) = turn.items.iter_mut().find(|item| item.id() == item_id) else {
        return;
    };
    append_delta_to_transcript_item(item, kind, delta);
    turn.rollout_end_index = rollout_index;
}

fn append_delta_to_transcript_item(
    item: &mut TranscriptItem,
    kind: &TurnItemDeltaKind,
    delta: &str,
) {
    match (item, kind) {
        (TranscriptItem::AgentMessage { text, .. }, TurnItemDeltaKind::Text)
        | (TranscriptItem::Reasoning { text, .. }, TurnItemDeltaKind::ReasoningSummary)
        | (TranscriptItem::Reasoning { text, .. }, TurnItemDeltaKind::ReasoningText) => {
            text.push_str(delta);
        }
        (
            TranscriptItem::CommandExecution {
                summary, stdout, ..
            },
            TurnItemDeltaKind::CommandExecutionOutput,
        ) => {
            if let Some(stdout) = stdout {
                stdout.push_str(delta);
            } else {
                *stdout = Some(delta.to_string());
            }
            summary.push_str(delta);
        }
        (TranscriptItem::FileChange { summary, .. }, TurnItemDeltaKind::FileChangeOutput) => {
            summary.push_str(delta);
        }
        (
            TranscriptItem::ToolResult {
                summary, content, ..
            },
            TurnItemDeltaKind::ToolOutput,
        ) => {
            summary.push_str(delta);
            content.push_str(delta);
        }
        _ => {}
    }
}

fn transcript_item_from_item_start(
    item_id: &str,
    kind: &TurnItemKind,
    title: Option<&str>,
) -> Option<TranscriptItem> {
    let title = title.unwrap_or_default().to_string();
    match kind {
        TurnItemKind::AssistantMessage => Some(TranscriptItem::AgentMessage {
            id: item_id.to_string(),
            text: String::new(),
        }),
        TurnItemKind::Reasoning => Some(TranscriptItem::Reasoning {
            id: item_id.to_string(),
            title: if title.is_empty() {
                "reasoning".to_string()
            } else {
                title
            },
            text: String::new(),
        }),
        TurnItemKind::CommandExecution => Some(TranscriptItem::CommandExecution {
            id: item_id.to_string(),
            tool_name: "exec_command".to_string(),
            command: title,
            current_directory: String::new(),
            status: crate::tool::CommandExecutionStatus::InProgress,
            exit_code: None,
            stdout: Some(String::new()),
            stderr: None,
            aggregated_output: None,
            duration_ms: None,
            summary: String::new(),
        }),
        TurnItemKind::FileChange => Some(TranscriptItem::FileChange {
            id: item_id.to_string(),
            tool_name: "apply_patch".to_string(),
            path: title,
            status: crate::tool::WriteFileStatus::InProgress,
            bytes_written: 0,
            summary: String::new(),
        }),
        TurnItemKind::ToolCall | TurnItemKind::ToolResult => Some(TranscriptItem::ToolResult {
            id: item_id.to_string(),
            tool_name: title,
            content: String::new(),
            summary: String::new(),
            structured: None,
        }),
        TurnItemKind::UserMessage | TurnItemKind::SystemNote => None,
    }
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

pub fn conversation_history_from_rollout_items(
    conversation_id: impl Into<String>,
    system_prompt: impl Into<String>,
    items: &[RolloutItem],
) -> crate::conversation::ConversationHistory {
    let mut history = crate::conversation::ConversationHistory::new(conversation_id, system_prompt);
    for item in items {
        match item {
            RolloutItem::ResponseItem { item } => match item {
                ResponseItem::System { content } => {
                    history.messages[0] = ResponseItem::System {
                        content: content.clone(),
                    };
                }
                ResponseItem::User { content } => {
                    history.turn_count += 1;
                    history.messages.push(ResponseItem::User {
                        content: content.clone(),
                    });
                }
                ResponseItem::Assistant {
                    content,
                    tool_calls,
                } => {
                    history.messages.push(ResponseItem::Assistant {
                        content: content.clone(),
                        tool_calls: tool_calls.clone(),
                    });
                }
                ResponseItem::Tool {
                    tool_call_id,
                    name,
                    content,
                    structured,
                } => {
                    history.messages.push(ResponseItem::Tool {
                        tool_call_id: tool_call_id.clone(),
                        name: name.clone(),
                        content: content.clone(),
                        structured: structured.clone(),
                    });
                }
            },
            RolloutItem::Compacted {
                replacement_history,
                ..
            } if !replacement_history.is_empty() => {
                history.messages = replacement_history.clone();
                history.turn_count = replacement_history
                    .iter()
                    .filter(|item| matches!(item, ResponseItem::User { .. }))
                    .count() as u64;
            }
            RolloutItem::EventMsg { .. } | RolloutItem::Compacted { .. } => {}
        }
    }
    history.ensure_tool_outputs_present();
    history
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
            aggregated_output,
            duration_ms,
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
            aggregated_output: aggregated_output.clone(),
            duration_ms: *duration_ms,
            summary: content.to_string(),
        },
        Some(StructuredToolResult::EditFile {
            changed_paths,
            files_changed,
            status,
        }) => TranscriptItem::FileChange {
            id: tool_call_id.to_string(),
            tool_name: name.to_string(),
            path: changed_paths.join(", "),
            status: status.clone(),
            bytes_written: *files_changed,
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
    fn active_turn_snapshot_projects_started_delta_before_completion() {
        let mut builder = ConversationHistoryBuilder::new();

        for item in [
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-1".to_string(),
                conversation_id: "default".to_string(),
                user_input: "hi".to_string(),
            }),
            RolloutItem::from(EventMsg::ItemStarted {
                turn_id: "turn-1".to_string(),
                item_id: "assistant:turn-1:0".to_string(),
                kind: TurnItemKind::AssistantMessage,
                title: Some("assistant_message".to_string()),
            }),
            RolloutItem::from(EventMsg::ItemDelta {
                turn_id: "turn-1".to_string(),
                item_id: "assistant:turn-1:0".to_string(),
                kind: TurnItemDeltaKind::Text,
                delta: "partial".to_string(),
            }),
        ] {
            builder.push_rollout_item(&item);
        }

        let snapshot = builder.active_turn_snapshot().expect("active turn");

        assert!(matches!(
            &snapshot.items[..],
            [
                TranscriptItem::UserMessage { text: user, .. },
                TranscriptItem::AgentMessage { text: assistant, .. },
            ] if user == "hi" && assistant == "partial"
        ));
    }

    #[test]
    fn item_completed_replaces_streamed_delta_projection() {
        let turns = build_turns_from_rollout_items(&[
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-1".to_string(),
                conversation_id: "default".to_string(),
                user_input: "hi".to_string(),
            }),
            RolloutItem::from(EventMsg::ItemStarted {
                turn_id: "turn-1".to_string(),
                item_id: "assistant:turn-1:0".to_string(),
                kind: TurnItemKind::AssistantMessage,
                title: Some("assistant_message".to_string()),
            }),
            RolloutItem::from(EventMsg::ItemDelta {
                turn_id: "turn-1".to_string(),
                item_id: "assistant:turn-1:0".to_string(),
                kind: TurnItemDeltaKind::Text,
                delta: "partial".to_string(),
            }),
            RolloutItem::from(EventMsg::ItemCompleted {
                turn_id: "turn-1".to_string(),
                item_id: "assistant:turn-1:0".to_string(),
                item: TranscriptItem::AgentMessage {
                    id: "assistant:turn-1:0".to_string(),
                    text: "final".to_string(),
                },
            }),
            RolloutItem::from(EventMsg::TurnCompleted {
                turn_id: "turn-1".to_string(),
            }),
        ]);

        assert!(matches!(
            &turns[0].items[..],
            [
                TranscriptItem::UserMessage { .. },
                TranscriptItem::AgentMessage { text, .. },
            ] if text == "final"
        ));
    }

    #[test]
    fn conversation_history_rebuilds_model_messages_from_rollout_response_items() {
        let history = conversation_history_from_rollout_items(
            "default",
            "system prompt",
            &[
                RolloutItem::from(ResponseItem::User {
                    content: "hi".to_string(),
                }),
                RolloutItem::from(ResponseItem::Assistant {
                    content: Some("hello".to_string()),
                    tool_calls: Vec::new(),
                }),
            ],
        );

        assert_eq!(history.id, "default");
        assert_eq!(history.turn_count, 1);
        assert!(matches!(
            &history.messages[..],
            [
                ResponseItem::System { content: system },
                ResponseItem::User { content: user },
                ResponseItem::Assistant {
                    content: Some(assistant),
                    ..
                },
            ] if system == "system prompt" && user == "hi" && assistant == "hello"
        ));
    }

    #[test]
    fn conversation_history_prefers_compacted_replacement_history() {
        let history = conversation_history_from_rollout_items(
            "default",
            "system prompt",
            &[
                RolloutItem::from(ResponseItem::User {
                    content: "old".to_string(),
                }),
                RolloutItem::Compacted {
                    summary: crate::context::CompactionSummary::from_model_output(
                        "Current Task:\n- old",
                    )
                    .ensure_defaults(),
                    rendered_summary: "[Context Summary]\nold".to_string(),
                    replacement_history: vec![
                        ResponseItem::System {
                            content: "system prompt".to_string(),
                        },
                        ResponseItem::System {
                            content: "[Context Summary]\nold".to_string(),
                        },
                        ResponseItem::User {
                            content: "latest".to_string(),
                        },
                        ResponseItem::Assistant {
                            content: Some("current".to_string()),
                            tool_calls: Vec::new(),
                        },
                    ],
                },
            ],
        );

        assert_eq!(history.turn_count, 1);
        assert!(matches!(
            &history.messages[..],
            [
                ResponseItem::System { content: system },
                ResponseItem::System { content: summary },
                ResponseItem::User { content: user },
                ResponseItem::Assistant {
                    content: Some(assistant),
                    ..
                },
            ] if system == "system prompt"
                && summary == "[Context Summary]\nold"
                && user == "latest"
                && assistant == "current"
        ));
    }

    #[test]
    fn transcript_builder_keeps_rich_tool_projection() {
        let item = transcript_item_from_response_item(&ResponseItem::Tool {
            tool_call_id: "call-1".to_string(),
            name: "exec_command".to_string(),
            content: "D:\\learn\\gifti\\cloudagent".to_string(),
            structured: Some(StructuredToolResult::CommandExecution {
                command: "pwd".to_string(),
                current_directory: "D:\\learn\\gifti\\cloudagent".to_string(),
                session_id: None,
                status: CommandExecutionStatus::Completed,
                exit_code: Some(0),
                success: Some(true),
                stdout: Some("D:\\learn\\gifti\\cloudagent".to_string()),
                stderr: Some(String::new()),
                aggregated_output: Some("D:\\learn\\gifti\\cloudagent".to_string()),
                duration_ms: Some(1),
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
            tool_name: "exec_command".to_string(),
            command: "pwd".to_string(),
            current_directory: "D:\\work".to_string(),
            status: CommandExecutionStatus::Completed,
            exit_code: Some(0),
            stdout: Some("D:\\work".to_string()),
            stderr: Some(String::new()),
            aggregated_output: Some("D:\\work".to_string()),
            duration_ms: Some(1),
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
