use crate::ui::history_cell::{HistoryCell, RenderContext, render_history_entry};
use agent_core::WriteFileStatus;
use agent_core::conversation::TranscriptItem;

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

    assert!(cell.body().contains("patched 5 files") || cell.body().contains("edited 5 files"));
    assert!(rendered.contains("a.rs"));
    assert!(rendered.contains("b.rs"));
    assert!(rendered.contains("+3 more files"));
    assert!(!rendered.contains("c.rs"));
    assert!(!rendered.contains("d.rs"));
    assert!(!rendered.contains("e.rs"));
}
