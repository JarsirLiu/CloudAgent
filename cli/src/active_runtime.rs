use agent_core::conversation::TranscriptItem;
use agent_core::{RuntimeItem, TurnItemKind};

pub(crate) fn started_live_label(kind: &TurnItemKind) -> Option<&'static str> {
    match kind {
        TurnItemKind::AssistantMessage => Some("Working"),
        TurnItemKind::Reasoning => Some("Thinking"),
        TurnItemKind::CommandExecution | TurnItemKind::ToolCall | TurnItemKind::ToolResult => {
            Some("Working")
        }
        _ => None,
    }
}

pub(crate) fn active_runtime_banner_text(
    item: &RuntimeItem,
    humanize_tool_label: impl Fn(&str) -> String,
) -> Option<String> {
    let title = item
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match item.kind {
        TurnItemKind::CommandExecution => Some(match title {
            Some(command) => format!("running command: {command}"),
            None => "running command".to_string(),
        }),
        TurnItemKind::ToolCall | TurnItemKind::ToolResult => Some(match title {
            Some(tool) => format!("executing tool: {}", humanize_tool_label(tool)),
            None => "executing tool".to_string(),
        }),
        _ => None,
    }
}

pub(crate) fn should_start_live_item(item: &RuntimeItem) -> bool {
    !matches!(item.kind, TurnItemKind::CommandExecution)
}

pub(crate) fn should_keep_completed_item_live(item: &TranscriptItem) -> bool {
    matches!(
        item,
        TranscriptItem::FileChange { .. } | TranscriptItem::ToolResult { .. }
    )
}

pub(crate) fn should_finish_active_runtime_item(item: &TranscriptItem) -> bool {
    should_keep_completed_item_live(item)
        || !matches!(
            item,
            TranscriptItem::CommandExecution {
                status: agent_core::CommandExecutionStatus::InProgress,
                ..
            }
        )
}
