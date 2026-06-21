use crate::conversation::{
    ConversationTurn, ResponseItem, TranscriptItem, input_items_to_plain_text,
};
use crate::rollout::RolloutItem;
use crate::runtime_item::RuntimeItem;
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
            // History compaction shapes model context, not the user-visible transcript.
            RolloutItem::Compacted { .. } => {}
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
        if matches!(item, ResponseItem::User { .. })
            && self
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.opened_explicitly)
        {
            return;
        }
        if self.merge_explicit_turn_assistant_response(item) {
            return;
        }
        if let Some(mut item) = transcript_item_from_response_item(item) {
            assign_response_item_id(&mut item, self.current_rollout_index);
            self.upsert_item_in_current_turn(item);
        }
    }

    fn merge_explicit_turn_assistant_response(&mut self, item: &ResponseItem) -> bool {
        let ResponseItem::Assistant {
            content: Some(content),
            ..
        } = item
        else {
            return false;
        };
        if content.trim().is_empty() {
            return false;
        }
        let Some(turn) = self
            .current_turn
            .as_mut()
            .filter(|turn| turn.opened_explicitly)
        else {
            return false;
        };
        let Some(index) = turn
            .items
            .iter()
            .rposition(|item| matches!(item, TranscriptItem::AgentMessage { .. }))
        else {
            return false;
        };
        if let TranscriptItem::AgentMessage { text, .. } = &mut turn.items[index] {
            *text = content.clone();
            turn.rollout_end_index = self.current_rollout_index;
            return true;
        }
        false
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
                self.upsert_item_in_current_turn(TranscriptItem::user_message(
                    format!("user:{turn_id}"),
                    user_input.clone(),
                ));
            }
            EventMsg::ItemStarted { turn_id, item, .. } => {
                if let Some(transcript_item) = transcript_item_from_item_start(item) {
                    self.upsert_item_in_turn_id_allow_empty(turn_id, transcript_item);
                }
            }
            EventMsg::ItemDelta {
                turn_id,
                item_id,
                kind,
                delta,
                ..
            } => {
                self.append_delta_to_item(turn_id, item_id, kind, delta);
            }
            EventMsg::ItemCompleted {
                turn_id,
                transcript_item,
                ..
            } => {
                self.upsert_item_in_turn_id(turn_id, transcript_item.clone());
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
            | EventMsg::ModelRetrying { .. }
            | EventMsg::TokenUsageUpdated { .. }
            | EventMsg::ContextCompacted { .. }
            | EventMsg::ContextCompactionStarted { .. }
            | EventMsg::ServerRequestRequested { .. }
            | EventMsg::ServerRequestResolved { .. }
            | EventMsg::ItemProgress { .. }
            | EventMsg::ItemMetricsUpdated { .. } => {}
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
                output,
                summary,
                ..
            } if *status == crate::tool::CommandExecutionStatus::InProgress => {
                *status = crate::tool::CommandExecutionStatus::Failed;
                *output = Some(reason.to_string());
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
                summary, output, ..
            },
            TurnItemDeltaKind::CommandExecutionOutput,
        ) => {
            if let Some(output) = output {
                output.push_str(delta);
            } else {
                *output = Some(delta.to_string());
            }
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

fn transcript_item_from_item_start(item: &RuntimeItem) -> Option<TranscriptItem> {
    let title = item.title.as_deref().unwrap_or_default().to_string();
    match item.kind {
        TurnItemKind::AssistantMessage => Some(TranscriptItem::AgentMessage {
            id: item.id.clone(),
            text: String::new(),
        }),
        TurnItemKind::Reasoning => Some(TranscriptItem::Reasoning {
            id: item.id.clone(),
            title: if title.is_empty() {
                "reasoning".to_string()
            } else {
                title
            },
            text: String::new(),
        }),
        TurnItemKind::CommandExecution => Some(TranscriptItem::CommandExecution {
            id: item.id.clone(),
            tool_name: "exec_command".to_string(),
            command: title,
            current_directory: String::new(),
            status: crate::tool::CommandExecutionStatus::InProgress,
            exit_code: None,
            output: Some(String::new()),
            duration_ms: None,
            summary: String::new(),
        }),
        TurnItemKind::FileChange => Some(TranscriptItem::FileChange {
            id: item.id.clone(),
            tool_name: "edit_file".to_string(),
            path: title,
            status: crate::tool::WriteFileStatus::InProgress,
            files_changed: 0,
            summary: String::new(),
        }),
        TurnItemKind::ToolCall | TurnItemKind::ToolResult => Some(TranscriptItem::ToolResult {
            id: item.id.clone(),
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
            runtime_items: Vec::new(),
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

pub fn filter_history_ui_turns(turns: Vec<ConversationTurn>) -> Vec<ConversationTurn> {
    turns
        .into_iter()
        .filter_map(filter_history_ui_turn)
        .collect()
}

pub fn filter_history_ui_turn(turn: ConversationTurn) -> Option<ConversationTurn> {
    let items = turn
        .items
        .into_iter()
        .filter(|item| !matches!(item, TranscriptItem::Reasoning { .. }))
        .collect::<Vec<_>>();
    if items.is_empty() {
        return None;
    }
    Some(ConversationTurn { items, ..turn })
}

pub fn transcript_item_from_response_item(message: &ResponseItem) -> Option<TranscriptItem> {
    match message {
        ResponseItem::System { content } => Some(TranscriptItem::SystemMessage {
            id: "system".to_string(),
            text: content.clone(),
        }),
        ResponseItem::User { content } => {
            Some(TranscriptItem::user_message(String::new(), content.clone()))
        }
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
            output,
            duration_ms,
            ..
        }) => TranscriptItem::CommandExecution {
            id: tool_call_id.to_string(),
            tool_name: name.to_string(),
            command: command.clone(),
            current_directory: current_directory.clone(),
            status: status.clone(),
            exit_code: *exit_code,
            output: output.clone(),
            duration_ms: *duration_ms,
            summary: content.to_string(),
        },
        Some(StructuredToolResult::EditFile {
            changed_paths,
            files_changed,
            status,
            ..
        }) => TranscriptItem::FileChange {
            id: tool_call_id.to_string(),
            tool_name: name.to_string(),
            path: changed_paths.join(", "),
            status: status.clone(),
            files_changed: *files_changed,
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
        | TranscriptItem::AgentMessage { text, .. }
        | TranscriptItem::Reasoning { text, .. } => text.trim().is_empty(),
        TranscriptItem::UserMessage { content, .. } => {
            input_items_to_plain_text(content).trim().is_empty()
        }
        TranscriptItem::CommandExecution { summary, .. }
        | TranscriptItem::FileChange { summary, .. }
        | TranscriptItem::ToolResult { summary, .. } => summary.trim().is_empty(),
    }
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
