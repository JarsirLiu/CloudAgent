use crate::projection::turn_projection_state::{ProjectedItemState, ProjectedItemStatus};
use agent_core::conversation::TranscriptItem;
use agent_core::{CommandExecutionStatus, TurnItemKind, WriteFileStatus};

pub(super) fn projected_item_from_transcript_item(
    turn_id: String,
    item: TranscriptItem,
    order_hint: usize,
) -> ProjectedItemState {
    let mut projected = base_projected_item(turn_id, item.id().to_string(), order_hint);
    match item {
        TranscriptItem::SystemMessage { text, .. } => {
            projected.kind = TurnItemKind::SystemNote;
            projected.summary = Some(text.clone());
            projected.text_buffer = text;
        }
        TranscriptItem::UserMessage { content, .. } => {
            projected.kind = TurnItemKind::UserMessage;
            projected.user_content = content;
        }
        TranscriptItem::AgentMessage { text, .. } => {
            projected.kind = TurnItemKind::AssistantMessage;
            projected.summary = Some(text.clone());
            projected.text_buffer = text;
        }
        TranscriptItem::Reasoning { title, text, .. } => {
            projected.kind = TurnItemKind::Reasoning;
            projected.title = Some(title);
            projected.summary = Some(text.clone());
            projected.reasoning_buffer = text;
            projected.reasoning_summary_part_opened = true;
        }
        TranscriptItem::CommandExecution {
            command, summary, output, ..
        } => {
            projected.kind = TurnItemKind::CommandExecution;
            projected.title = Some(command);
            projected.summary = Some(summary.clone());
            projected.tool_output_buffer = output.unwrap_or_default();
            projected.text_buffer.clear();
        }
        TranscriptItem::FileChange { path, summary, .. } => {
            projected.kind = TurnItemKind::FileChange;
            projected.title = Some(path);
            projected.summary = Some(summary.clone());
            projected.tool_output_buffer = summary;
        }
        TranscriptItem::ToolResult {
            tool_name,
            content,
            summary,
            structured,
            ..
        } => {
            projected.kind = TurnItemKind::ToolResult;
            projected.title = Some(tool_name);
            projected.summary = Some(summary.clone());
            projected.structured = structured;
            projected.tool_output_buffer = content;
        }
    }

    projected
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
            summary: item
                .summary
                .clone()
                .unwrap_or_else(|| item.tool_output_buffer.clone()),
        }),
        TurnItemKind::FileChange => Some(TranscriptItem::FileChange {
            id: item.item_id.clone(),
            tool_name: "edit_file".to_string(),
            path: item.title.clone().unwrap_or_default(),
            status: WriteFileStatus::InProgress,
            files_changed: 0,
            summary: item
                .summary
                .clone()
                .unwrap_or_else(|| item.tool_output_buffer.clone()),
        }),
        TurnItemKind::ToolCall | TurnItemKind::ToolResult => Some(TranscriptItem::ToolResult {
            id: item.item_id.clone(),
            tool_name: item.title.clone().unwrap_or_else(|| "tool".to_string()),
            content: item.tool_output_buffer.clone(),
            summary: item.summary.clone().unwrap_or_default(),
            structured: item.structured.clone(),
        }),
        TurnItemKind::SystemNote => Some(TranscriptItem::SystemMessage {
            id: item.item_id.clone(),
            text: item.text_buffer.clone(),
        }),
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

fn base_projected_item(
    turn_id: String,
    item_id: String,
    order_hint: usize,
) -> ProjectedItemState {
    ProjectedItemState {
        turn_id,
        item_id,
        call_id: None,
        kind: TurnItemKind::SystemNote,
        title: None,
        summary: None,
        tool_identity: None,
        structured: None,
        progress: None,
        metrics: None,
        status: ProjectedItemStatus::Completed,
        last_delta_kind: None,
        user_content: Vec::new(),
        text_buffer: String::new(),
        reasoning_buffer: String::new(),
        tool_output_buffer: String::new(),
        patch_buffer: String::new(),
        reasoning_summary_part_opened: false,
        order_hint: order_hint as u64,
    }
}
