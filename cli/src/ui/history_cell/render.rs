use super::patch;
use super::tool_ui;
use super::{HistoryCell, HistoryFormat, HistoryTone};
use agent_core::{InputItem, RuntimeItem, TranscriptItem, TurnItemKind};

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
        } => patch::render_patch_result(tool_name, content, structured.as_ref()).unwrap_or_else(
            || tool_ui::render_tool_result(tool_name, content, structured.as_ref()),
        ),
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
        } => patch::render_file_change(tool_name, path, status, *files_changed, summary),
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

pub fn render_active_runtime_item(item: &RuntimeItem) -> HistoryCell {
    match item.kind {
        TurnItemKind::AssistantMessage => {
            HistoryCell::agent("", "responding".to_string(), HistoryFormat::Markdown)
        }
        TurnItemKind::Reasoning => HistoryCell::reasoning("Reasoning", "thinking".to_string()),
        _ => tool_ui::render_active_runtime_item(item),
    }
}

pub(crate) fn humanize_tool_label(tool_name: &str) -> String {
    tool_ui::humanize_tool_label(tool_name)
}
