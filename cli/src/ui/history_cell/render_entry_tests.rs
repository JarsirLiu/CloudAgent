use crate::ui::history_cell::HistoryTone;
use crate::ui::history_cell::{HistoryCell, RenderContext, render_history_entry};
use agent_core::conversation::{AttachmentRef, TranscriptItem};
use agent_core::{CommandExecutionStatus, StructuredToolResult, WebSearchAction};

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
