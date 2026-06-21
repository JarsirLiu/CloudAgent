use crate::projection::turn_projection_state::{ProjectedItemState, ProjectedItemStatus};
use agent_core::conversation::TranscriptItem;
use agent_core::{
    CommandExecutionStatus, StructuredToolResult, TurnItemKind, WriteFileStatus, web_search_detail,
};

pub(super) fn projected_item_from_transcript_item(
    turn_id: String,
    item: TranscriptItem,
    order_hint: usize,
) -> ProjectedItemState {
    let kind = turn_item_kind_for_transcript_item(&item);
    match item {
        TranscriptItem::SystemMessage { id, text } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: None,
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: text,
            reasoning_buffer: String::new(),
            tool_output_buffer: String::new(),
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
        TranscriptItem::UserMessage { id, content } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: None,
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: content.clone(),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: String::new(),
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
        TranscriptItem::AgentMessage { id, text } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: None,
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: text,
            reasoning_buffer: String::new(),
            tool_output_buffer: String::new(),
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
        TranscriptItem::Reasoning { id, title, text } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: Some(title),
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: String::new(),
            reasoning_buffer: text,
            tool_output_buffer: String::new(),
            reasoning_summary_part_opened: true,
            order_hint: order_hint as u64,
        },
        TranscriptItem::CommandExecution {
            id,
            tool_name: _,
            command,
            current_directory: _,
            status: _,
            exit_code: _,
            output: _,
            duration_ms: _,
            summary,
        } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: Some(command),
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: summary,
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
        TranscriptItem::FileChange {
            id,
            tool_name: _,
            path,
            status: _,
            files_changed: _,
            summary,
        } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: Some(path),
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: summary,
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
        TranscriptItem::ToolResult {
            id,
            tool_name,
            content,
            summary,
            structured,
        } => ProjectedItemState {
            turn_id,
            item_id: id,
            call_id: None,
            kind,
            title: Some(tool_name),
            status: ProjectedItemStatus::Completed,
            last_delta_kind: None,
            user_content: Vec::new(),
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            tool_output_buffer: match structured.as_ref() {
                Some(StructuredToolResult::WebSearch { query, action, .. }) => {
                    web_search_detail(query, action.as_ref())
                }
                _ if summary.trim().is_empty() => content,
                _ => summary,
            },
            reasoning_summary_part_opened: false,
            order_hint: order_hint as u64,
        },
    }
}

pub(super) fn projected_item_to_transcript_item(
    item: &ProjectedItemState,
) -> Option<TranscriptItem> {
    match item.kind {
        TurnItemKind::UserMessage => Some(TranscriptItem::UserMessage {
            id: item.item_id.clone(),
            content: item.user_content.clone(),
        }),
        TurnItemKind::AssistantMessage => Some(TranscriptItem::AgentMessage {
            id: item.item_id.clone(),
            text: item.text_buffer.clone(),
        }),
        TurnItemKind::Reasoning => Some(TranscriptItem::Reasoning {
            id: item.item_id.clone(),
            title: item
                .title
                .clone()
                .unwrap_or_else(|| "reasoning".to_string()),
            text: item.reasoning_buffer.clone(),
        }),
        TurnItemKind::CommandExecution => Some(TranscriptItem::CommandExecution {
            id: item.item_id.clone(),
            tool_name: "exec_command".to_string(),
            command: item.title.clone().unwrap_or_default(),
            current_directory: String::new(),
            status: CommandExecutionStatus::InProgress,
            exit_code: None,
            output: Some(item.tool_output_buffer.clone()),
            duration_ms: None,
            summary: item.tool_output_buffer.clone(),
        }),
        TurnItemKind::FileChange => Some(TranscriptItem::FileChange {
            id: item.item_id.clone(),
            tool_name: "edit_file".to_string(),
            path: item.title.clone().unwrap_or_default(),
            status: WriteFileStatus::InProgress,
            files_changed: 0,
            summary: item.tool_output_buffer.clone(),
        }),
        TurnItemKind::ToolCall | TurnItemKind::ToolResult => Some(TranscriptItem::ToolResult {
            id: item.item_id.clone(),
            tool_name: item.title.clone().unwrap_or_else(|| "tool".to_string()),
            content: item.tool_output_buffer.clone(),
            summary: item.tool_output_buffer.clone(),
            structured: None,
        }),
        TurnItemKind::SystemNote => Some(TranscriptItem::SystemMessage {
            id: item.item_id.clone(),
            text: item.text_buffer.clone(),
        }),
    }
}

pub(super) fn fallback_started_item(
    item_id: &str,
    kind: &TurnItemKind,
    title: Option<&str>,
) -> TranscriptItem {
    match projected_item_to_transcript_item(&ProjectedItemState::new(
        String::new(),
        item_id.to_string(),
        None,
        kind.clone(),
        title.map(str::to_string),
        0,
    )) {
        Some(item) => item,
        None => TranscriptItem::SystemMessage {
            id: item_id.to_string(),
            text: String::new(),
        },
    }
}

pub(super) fn turn_item_kind_for_transcript_item(item: &TranscriptItem) -> TurnItemKind {
    match item {
        TranscriptItem::SystemMessage { .. } => TurnItemKind::SystemNote,
        TranscriptItem::UserMessage { .. } => TurnItemKind::UserMessage,
        TranscriptItem::AgentMessage { .. } => TurnItemKind::AssistantMessage,
        TranscriptItem::CommandExecution { .. } => TurnItemKind::CommandExecution,
        TranscriptItem::FileChange { .. } => TurnItemKind::FileChange,
        TranscriptItem::ToolResult { .. } => TurnItemKind::ToolResult,
        TranscriptItem::Reasoning { .. } => TurnItemKind::Reasoning,
    }
}

pub(super) fn projected_transcript_item_is_empty(item: &TranscriptItem) -> bool {
    match item {
        TranscriptItem::SystemMessage { text, .. }
        | TranscriptItem::AgentMessage { text, .. }
        | TranscriptItem::Reasoning { text, .. } => text.trim().is_empty(),
        TranscriptItem::UserMessage { content, .. } => {
            agent_core::input_items_to_plain_text(content)
                .trim()
                .is_empty()
        }
        TranscriptItem::CommandExecution { summary, .. }
        | TranscriptItem::FileChange { summary, .. }
        | TranscriptItem::ToolResult { summary, .. } => summary.trim().is_empty(),
    }
}
