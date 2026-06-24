use super::active_runtime::{
    active_runtime_banner_text, should_keep_completed_item_live, should_start_live_item,
    started_live_label,
};
use agent_core::conversation::TranscriptItem;
use agent_core::{RuntimeItem, TurnItemKind};

#[test]
fn started_live_label_collapses_command_and_tool_items_into_working() {
    assert_eq!(
        started_live_label(&TurnItemKind::CommandExecution),
        Some("Working")
    );
    assert_eq!(started_live_label(&TurnItemKind::ToolCall), Some("Working"));
    assert_eq!(
        started_live_label(&TurnItemKind::Reasoning),
        Some("Thinking")
    );
}

#[test]
fn active_runtime_banner_text_keeps_command_and_tool_banners_separate() {
    let command = RuntimeItem::started(
        "item-1",
        None,
        TurnItemKind::CommandExecution,
        Some("git status".to_string()),
    );
    let tool = RuntimeItem::started(
        "item-2",
        None,
        TurnItemKind::ToolCall,
        Some("web_search".to_string()),
    );

    assert_eq!(
        active_runtime_banner_text(&command, |tool_name| tool_name.to_string()),
        Some("running command: git status".to_string())
    );
    assert_eq!(
        active_runtime_banner_text(&tool, |tool_name| tool_name.to_string()),
        Some("executing tool: web_search".to_string())
    );
}

#[test]
fn live_item_rules_stay_focused_on_transient_runtime_cells() {
    let command = RuntimeItem::started("item-1", None, TurnItemKind::CommandExecution, None);
    let tool_result = TranscriptItem::ToolResult {
        id: "item-2".to_string(),
        tool_name: "web_search".to_string(),
        content: String::new(),
        summary: String::new(),
        structured: None,
    };

    assert!(!should_start_live_item(&command));
    assert!(should_keep_completed_item_live(&tool_result));
}
