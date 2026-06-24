use crate::state::bottom_pane_runtime::BottomPaneRuntimeState;
use agent_core::{RuntimeItem, RuntimeItemProgress, TurnItemKind};

#[test]
fn command_start_sets_working_label_without_active_banner() {
    let mut runtime = BottomPaneRuntimeState::default();
    let item = RuntimeItem::started(
        "cmd-1",
        None,
        TurnItemKind::CommandExecution,
        Some("git status".to_string()),
    );

    runtime.on_active_item_started(&item);

    assert_eq!(
        runtime.display_banner_text(),
        Some("running command: git status".to_string())
    );
}

#[test]
fn tool_start_keeps_active_banner_and_progress_text() {
    let mut runtime = BottomPaneRuntimeState::default();
    let item = RuntimeItem::started(
        "tool-1",
        None,
        TurnItemKind::ToolCall,
        Some("web_search".to_string()),
    )
    .with_progress(RuntimeItemProgress::message("weather seattle"));

    runtime.on_active_item_started(&item);

    assert_eq!(
        runtime.display_banner_text(),
        Some("executing tool: Web search · weather seattle".to_string())
    );
}
