use crate::ui::history_cell::HistoryTone;
use crate::ui::history_cell::{HistoryCell, RenderContext, render_history_entry};
use agent_core::conversation::{AttachmentRef, TranscriptItem};
use agent_core::{CommandExecutionStatus, StructuredToolResult, WebSearchAction, WriteFileStatus};

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
fn user_message_renders_image_placeholders_in_history() {
    let message = TranscriptItem::user_message(
        "user-1",
        vec![
            agent_core::InputItem::Text {
                text: "please inspect".to_string(),
            },
            agent_core::InputItem::Image {
                source: AttachmentRef::LocalPath {
                    path: "D:\\images\\diagram.png".to_string(),
                },
                detail: None,
                alt: None,
            },
            agent_core::InputItem::Image {
                source: AttachmentRef::LocalPath {
                    path: "D:\\images\\diagram-2.png".to_string(),
                },
                detail: None,
                alt: None,
            },
        ],
    );

    let mut context = RenderContext;
    let cell = render_history_entry(&message, &mut context);

    assert_eq!(cell.body(), "[Image #1]\n[Image #2]\n\nplease inspect");
}

#[test]
fn failed_file_change_does_not_render_full_patch_error() {
    let message = TranscriptItem::FileChange {
        id: "tool-1".to_string(),
        tool_name: "apply_patch".to_string(),
        path: String::new(),
        status: WriteFileStatus::Failed,
        files_changed: 0,
        summary: "Tool execution failed: failed to apply patch for file.rs: Failed to find expected lines:\n*** Begin Patch\n*** Update File: file.rs\n@@\n-old\n+new\n*** End Patch".to_string(),
    };

    let mut context = RenderContext;
    let cell = render_history_entry(&message, &mut context);
    let rendered = joined(&cell, 120);

    assert!(!rendered.contains("*** Begin Patch"));
    assert!(cell.body().contains("failed 0 files"));
    assert!(rendered.contains("expected lines not found"));
    assert!(!rendered.contains("Tool execution failed"));
}

#[test]
fn file_change_renders_bounded_path_details() {
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
    let rendered = joined(&cell, 120);

    assert!(cell.body().contains("edited 5 files"));
    assert!(rendered.contains("a.rs"));
    assert!(rendered.contains("b.rs"));
    assert!(rendered.contains("+3 more files"));
    assert!(!rendered.contains("c.rs"));
    assert!(!rendered.contains("d.rs"));
    assert!(!rendered.contains("e.rs"));
}

#[test]
fn empty_write_stdin_poll_does_not_render_history_cell() {
    let message = TranscriptItem::CommandExecution {
        id: "tool-1".to_string(),
        tool_name: "write_stdin".to_string(),
        command: "Get-Content slow.log".to_string(),
        current_directory: "D:\\work".to_string(),
        status: CommandExecutionStatus::InProgress,
        exit_code: None,
        output: Some(String::new()),
        duration_ms: Some(250),
        summary: String::new(),
    };

    let mut context = RenderContext;
    let cell = render_history_entry(&message, &mut context);

    assert!(cell.is_empty());
}

#[test]
fn web_search_renders_as_independent_exploration_cell() {
    let message = TranscriptItem::ToolResult {
        id: "ws_1".to_string(),
        tool_name: "web_search".to_string(),
        content: "OpenAI latest API pricing".to_string(),
        summary: "searched the web".to_string(),
        structured: Some(StructuredToolResult::WebSearch {
            query: String::new(),
            action: Some(WebSearchAction::Search {
                query: Some("OpenAI latest API pricing".to_string()),
                queries: None,
            }),
            result_count: None,
            source_count: None,
        }),
    };

    let mut context = RenderContext;
    let cell = render_history_entry(&message, &mut context);
    let rendered = joined(&cell, 120);

    assert_eq!(cell.label(), "Web search");
    assert_eq!(cell.tone, HistoryTone::Control);
    assert!(rendered.contains("Web search"));
    assert!(rendered.contains("OpenAI latest API pricing"));
}
