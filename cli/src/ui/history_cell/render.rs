use super::tool_ui;
use super::{HistoryCell, HistoryFormat, HistoryTone};
use agent_core::{InputItem, TranscriptItem, TurnItemKind};

#[derive(Default)]
pub struct RenderContext;

pub fn render_history_entry(message: &TranscriptItem, context: &mut RenderContext) -> HistoryCell {
    match message {
        TranscriptItem::SystemMessage { .. } => HistoryCell::info("", "", HistoryTone::Meta),
        TranscriptItem::UserMessage { content, .. } => {
            HistoryCell::user(render_user_content(content))
        }
        TranscriptItem::AgentMessage { text, .. } => {
            let _ = context;
            HistoryCell::agent("", text.clone(), HistoryFormat::Markdown)
        }
        TranscriptItem::ToolResult {
            tool_name,
            content,
            structured,
            ..
        } => tool_ui::render_tool_result(tool_name, content, structured.as_ref()),
        TranscriptItem::CommandExecution {
            tool_name,
            command,
            current_directory,
            status,
            exit_code,
            output,
            summary,
            ..
        } => tool_ui::render_command_execution(
            tool_name,
            command,
            current_directory,
            status,
            *exit_code,
            output.as_deref().or(Some(summary.as_str())),
        ),
        TranscriptItem::FileChange {
            tool_name,
            path,
            status,
            files_changed,
            summary,
            ..
        } => tool_ui::render_file_change(tool_name, path, status, *files_changed, summary),
        TranscriptItem::Reasoning { text, .. } => HistoryCell::reasoning("Reasoning", text.clone()),
    }
}

fn render_user_content(content: &[InputItem]) -> String {
    let mut media_lines = Vec::new();
    let mut text_lines = Vec::new();
    let mut image_index = 0usize;
    for item in content {
        match item {
            InputItem::Text { text } => {
                if !text.trim().is_empty() {
                    text_lines.push(text.trim().to_string());
                }
            }
            InputItem::Image { .. } => {
                image_index += 1;
                media_lines.push(format!("[Image #{image_index}]"));
            }
            InputItem::File { .. } => media_lines.push("[Attachment]".to_string()),
            InputItem::Mention { name, path } => text_lines.push(format!("@{name} ({path})")),
            InputItem::Skill { name, path } => text_lines.push(format!("${name} ({path})")),
        }
    }

    match (media_lines.is_empty(), text_lines.is_empty()) {
        (false, false) => format!("{}\n\n{}", media_lines.join("\n"), text_lines.join("\n")),
        (false, true) => media_lines.join("\n"),
        (true, false) => text_lines.join("\n"),
        (true, true) => String::new(),
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use agent_core::conversation::{AttachmentRef, TranscriptItem};
    use agent_core::{CommandExecutionStatus, WriteFileStatus};

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
                InputItem::Text {
                    text: "please inspect".to_string(),
                },
                InputItem::Image {
                    source: AttachmentRef::LocalPath {
                        path: "D:\\images\\diagram.png".to_string(),
                    },
                    detail: None,
                    alt: None,
                },
                InputItem::Image {
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
}

pub fn render_active_item_placeholder(kind: TurnItemKind, title: &str) -> HistoryCell {
    match kind {
        TurnItemKind::AssistantMessage => {
            HistoryCell::agent("", "responding".to_string(), HistoryFormat::Markdown)
        }
        TurnItemKind::Reasoning => HistoryCell::reasoning("Reasoning", "thinking".to_string()),
        _ => tool_ui::render_active_placeholder(kind, title),
    }
}

pub(crate) fn humanize_tool_label(tool_name: &str) -> String {
    tool_ui::humanize_tool_label(tool_name)
}
