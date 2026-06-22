use super::tool_common::{compact_inline, compact_path, humanize_tool_label};
use super::{HistoryCell, HistoryTone};
use crate::runtime_metrics_display::format_runtime_metrics;
use agent_core::{RuntimeItem, StructuredToolResult, TurnItemKind, WriteFileStatus};

pub(super) fn render_patch_placeholder(tool_name: &str) -> HistoryCell {
    HistoryCell::patch(
        humanize_tool_label(tool_name),
        "running",
        None,
        HistoryTone::Control,
    )
}

pub(super) fn render_active_runtime_item(item: &RuntimeItem) -> HistoryCell {
    let title = item.title.as_deref().unwrap_or("");
    let mut cell = render_patch_placeholder(title);

    if let Some(summary) = item
        .progress
        .as_ref()
        .and_then(|progress| progress.message.clone())
        .or_else(|| item.summary.clone())
        && !summary.trim().is_empty()
    {
        cell.replace_body(summary);
    }

    if let Some(detail) = item.metrics.as_ref().and_then(format_runtime_metrics)
        && matches!(
            item.kind,
            TurnItemKind::FileChange | TurnItemKind::ToolResult
        )
    {
        cell.append_detail(&detail);
    }

    cell
}

pub(super) fn render_patch_result(
    tool_name: &str,
    structured: Option<&StructuredToolResult>,
) -> Option<HistoryCell> {
    let StructuredToolResult::EditFile {
        changed_paths,
        files_changed,
        status,
        ..
    } = structured?
    else {
        return None;
    };

    let detail = format_changed_paths_detail(changed_paths.iter().map(String::as_str));
    let status = status.clone();
    Some(render_patch_cell(
        humanize_tool_label(tool_name),
        format_patch_summary(status.clone(), *files_changed),
        detail,
        status,
    ))
}

pub(super) fn render_file_change(
    tool_name: &str,
    path: &str,
    status: &WriteFileStatus,
    files_changed: usize,
    summary: &str,
) -> HistoryCell {
    let detail = format_changed_paths_detail(path.split(',').map(str::trim)).or_else(|| {
        (matches!(status, WriteFileStatus::Failed | WriteFileStatus::Declined)
            && !summary.trim().is_empty())
        .then(|| compact_patch_failure(summary))
    });
    let status = status.clone();
    render_patch_cell(
        humanize_tool_label(tool_name),
        format_patch_summary(status.clone(), files_changed),
        detail,
        status,
    )
}

fn render_patch_cell(
    label: String,
    summary: String,
    detail: Option<String>,
    status: WriteFileStatus,
) -> HistoryCell {
    let tone = match status {
        WriteFileStatus::Failed => HistoryTone::Error,
        WriteFileStatus::Declined => HistoryTone::Warning,
        _ => HistoryTone::Control,
    };
    HistoryCell::patch(label, summary, detail, tone)
}

fn format_patch_summary(status: WriteFileStatus, files_changed: usize) -> String {
    let verb = match status {
        WriteFileStatus::InProgress => "editing",
        WriteFileStatus::Completed => "patched",
        WriteFileStatus::Declined => "declined",
        WriteFileStatus::Failed => "failed",
    };
    format!("{verb} {files_changed} files")
}

fn format_changed_paths_detail<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let paths = paths
        .into_iter()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return None;
    }

    let visible_paths = if paths.len() > 3 { 2 } else { 3 };
    let mut lines = paths
        .iter()
        .take(visible_paths)
        .map(|path| compact_path(path, 64))
        .collect::<Vec<_>>();
    if paths.len() > visible_paths {
        lines.push(format!("+{} more files", paths.len() - visible_paths));
    }
    Some(lines.join("\n"))
}

fn compact_patch_failure(summary: &str) -> String {
    let text = summary.trim();
    if text.contains("Failed to find expected lines") {
        return "expected lines not found".to_string();
    }
    if text.contains("patch did not contain any editable file hunks") {
        return "invalid patch format".to_string();
    }
    if text.contains("refusing to add existing file") {
        return "file already exists".to_string();
    }
    if text.contains("refusing to update missing file") {
        return "file does not exist".to_string();
    }
    if text.contains("partial_changes=") {
        return compact_partial_change_failure(text);
    }
    let first = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("patch failed");
    let first = first
        .strip_prefix("Tool execution failed:")
        .unwrap_or(first)
        .trim();
    let first = first
        .strip_prefix("apply_patch failed:")
        .unwrap_or(first)
        .trim();
    compact_inline(first, 72)
}

fn compact_partial_change_failure(summary: &str) -> String {
    summary
        .lines()
        .find(|line| line.contains("partial_changes="))
        .map(|line| compact_inline(line.trim(), 72))
        .unwrap_or_else(|| "partial changes may have been written".to_string())
}
