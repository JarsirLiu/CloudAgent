use super::render_tool_result;
use crate::ui::history_cell::{HistoryCell, HistoryTone, RenderContext, render_history_entry};
use agent_core::conversation::TranscriptItem;
use agent_core::{
    SearchWorkspaceMode, SearchWorkspaceOperation, SearchWorkspaceStatus, StructuredToolResult,
    WebSearchAction, WriteFileStatus,
};

fn joined(cell: &HistoryCell, width: usize) -> String {
    cell.to_lines_with_mode(width)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn search_workspace_renders_as_exploration_card() {
    let cell = render_tool_result(
        "search_workspace",
        "",
        Some(&StructuredToolResult::SearchWorkspace {
            session_id: "session-1".to_string(),
            operation: SearchWorkspaceOperation::Search,
            mode: SearchWorkspaceMode::Text,
            status: SearchWorkspaceStatus::Closed,
            query: "codex item cards".to_string(),
            path_scope: None,
            case_sensitive: false,
            context_lines: 2,
            max_results: 20,
            offset: 0,
            file_count: 4,
            match_count: 8,
            truncated: false,
            next_offset: None,
            hits: vec![],
        }),
    );

    assert_eq!(cell.label(), "Search workspace");
    assert_eq!(cell.tone, HistoryTone::Control);
    assert!(cell.body().contains("matched 8 hits"));
}

#[test]
fn tool_search_renders_as_search_card() {
    let cell = render_tool_result(
        "tool_search",
        "",
        Some(&StructuredToolResult::ToolSearch {
            query: "history cell".to_string(),
            max_results: 20,
            match_count: 3,
            hits: vec![],
        }),
    );

    assert_eq!(cell.label(), "Search tools");
    assert!(cell.body().contains("matched 3 tools"));
}

#[test]
fn web_search_renders_detail_as_search_card() {
    let cell = render_tool_result(
        "web_search",
        "OpenAI latest API pricing",
        Some(&StructuredToolResult::WebSearch {
            query: String::new(),
            action: Some(WebSearchAction::Search {
                query: Some("OpenAI latest API pricing".to_string()),
                queries: None,
            }),
            result_count: Some(3),
            source_count: Some(2),
        }),
    );

    assert_eq!(cell.label(), "Web search");
    assert!(cell.body().contains("searched the web"));
    assert!(joined(&cell, 120).contains("OpenAI latest API pricing"));
}

#[test]
fn file_change_rendering_stays_in_patch_module() {
    let message = TranscriptItem::FileChange {
        id: "tool-1".to_string(),
        tool_name: "apply_patch".to_string(),
        path: "a.rs, b.rs, c.rs, d.rs, e.rs".to_string(),
        status: WriteFileStatus::Completed,
        files_changed: 5,
        summary: "Applied patch.".to_string(),
    };

    let mut context = RenderContext;
    let cell = render_history_entry(&message, &mut context);

    assert_eq!(cell.label(), "Edit file");
    assert!(cell.body().contains("patched 5 files") || cell.body().contains("edited 5 files"));
}
