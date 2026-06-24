use super::tool_common::{compact_inline, compact_path, format_line_range, humanize_tool_label};
use super::{ExplorationAggregate, HistoryCell, HistoryTone};
use crate::runtime_metrics_display::format_runtime_metrics;
use agent_core::{
    RuntimeItem, SearchWorkspaceMode, SearchWorkspaceStatus, StructuredToolResult, TurnItemKind,
    WebSearchAction,
    web_search_presentation::web_search_presentation as web_search_card_presentation,
};

pub(super) fn render_active_placeholder(kind: TurnItemKind, title: &str) -> HistoryCell {
    match kind {
        TurnItemKind::ToolCall | TurnItemKind::ToolResult => HistoryCell::info(
            humanize_tool_label(title),
            "running".to_string(),
            HistoryTone::Control,
        ),
        _ => HistoryCell::info(
            title.to_string(),
            "running".to_string(),
            HistoryTone::Control,
        ),
    }
}

pub(super) fn render_active_runtime_item(item: &RuntimeItem) -> HistoryCell {
    let title = item.title.as_deref().unwrap_or("");
    let mut cell = render_active_placeholder(item.kind.clone(), title);

    let summary = match item.structured.as_ref() {
        Some(StructuredToolResult::WebSearch {
            query,
            action,
            result_count,
            source_count,
        }) => {
            return build_web_search_cell(query, action.as_ref(), *source_count, *result_count);
        }
        _ => runtime_summary(item),
    };

    if let Some(summary) = summary
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

pub(super) fn render_tool_result(
    tool_name: &str,
    content: &str,
    structured: Option<&StructuredToolResult>,
) -> HistoryCell {
    if let Some(cell) = render_command_result(tool_name, structured) {
        return cell;
    }
    if let Some(cell) = render_read_file_result(structured) {
        return cell;
    }
    if let Some(cell) = render_search_workspace_result(structured) {
        return cell;
    }
    if let Some(cell) = render_tool_search_result(structured) {
        return cell;
    }
    if let Some(cell) = render_read_directory_result(structured) {
        return cell;
    }
    if let Some(cell) = render_metadata_result(tool_name, structured) {
        return cell;
    }
    if let Some(cell) = render_web_search_result(structured) {
        return cell;
    }
    if let Some(cell) = render_tool_error_result(tool_name, structured) {
        return cell;
    }

    let first = first_non_empty_line(content);
    HistoryCell::info(
        humanize_tool_label(tool_name),
        compact_inline(first, 100),
        HistoryTone::Control,
    )
}

fn render_command_result(
    tool_name: &str,
    structured: Option<&StructuredToolResult>,
) -> Option<HistoryCell> {
    let Some(StructuredToolResult::CommandExecution {
        command,
        current_directory,
        status,
        exit_code,
        output,
        ..
    }) = structured
    else {
        return None;
    };

    Some(super::command::render_command_execution(
        tool_name,
        command,
        current_directory,
        status,
        *exit_code,
        output.as_deref(),
    ))
}

fn render_read_file_result(structured: Option<&StructuredToolResult>) -> Option<HistoryCell> {
    let Some(StructuredToolResult::ReadFile {
        path,
        total_chars,
        read,
        ..
    }) = structured
    else {
        return None;
    };

    let display_path = compact_path(path, 56);
    let range_suffix = format_line_range(read.start_line, read.end_line);
    let detail = format!(
        "{}{} 鈥?{} chars{}",
        display_path,
        range_suffix,
        total_chars,
        if read.truncated { " truncated" } else { "" }
    );
    let mut aggregate = ExplorationAggregate::new(detail);
    aggregate.read_files = 1;
    Some(HistoryCell::exploration(
        "Read file",
        "read 1 file".to_string(),
        aggregate,
        HistoryTone::Control,
    ))
}

fn render_search_workspace_result(
    structured: Option<&StructuredToolResult>,
) -> Option<HistoryCell> {
    let Some(StructuredToolResult::SearchWorkspace {
        mode,
        status,
        query,
        file_count,
        match_count,
        truncated,
        ..
    }) = structured
    else {
        return None;
    };

    let status_suffix = search_status_suffix(status);
    let truncation = if *truncated { " truncated" } else { "" };
    let summary = match mode {
        SearchWorkspaceMode::Files => {
            format!("found {file_count} files{truncation}{status_suffix}")
        }
        SearchWorkspaceMode::Text => {
            format!("matched {match_count} hits in {file_count} files{truncation}{status_suffix}")
        }
    };
    let detail = format_search_workspace_detail(
        query,
        mode.clone(),
        status.clone(),
        *file_count,
        *match_count,
        *truncated,
    );
    Some(HistoryCell::search(
        "Search workspace",
        summary,
        Some(detail),
        HistoryTone::Control,
    ))
}

fn render_tool_search_result(structured: Option<&StructuredToolResult>) -> Option<HistoryCell> {
    let Some(StructuredToolResult::ToolSearch {
        query,
        max_results,
        match_count,
        hits,
    }) = structured
    else {
        return None;
    };

    let detail = format_tool_search_detail(query, *match_count, hits.len(), *max_results);
    Some(HistoryCell::search(
        "Search tools",
        format!("matched {match_count} tools"),
        Some(detail),
        HistoryTone::Control,
    ))
}

fn render_read_directory_result(structured: Option<&StructuredToolResult>) -> Option<HistoryCell> {
    let Some(StructuredToolResult::ReadDirectory {
        path,
        entry_count,
        truncated,
        ..
    }) = structured
    else {
        return None;
    };

    let mut aggregate = ExplorationAggregate::new(format!(
        "{} 鈥?{} entries{}",
        compact_path(path, 56),
        entry_count,
        if *truncated { " truncated" } else { "" }
    ));
    aggregate.listed_directories = 1;
    Some(HistoryCell::exploration(
        "Read directory",
        "listed 1 directory".to_string(),
        aggregate,
        HistoryTone::Control,
    ))
}

fn render_metadata_result(
    tool_name: &str,
    structured: Option<&StructuredToolResult>,
) -> Option<HistoryCell> {
    let Some(StructuredToolResult::GetMetadata {
        path,
        size,
        exists,
        is_file,
        ..
    }) = structured
    else {
        return None;
    };

    if *exists {
        let kind = if *is_file { "file" } else { "directory" };
        let mut aggregate = ExplorationAggregate::new(format!(
            "metadata {} 鈥?{kind} ({size} bytes)",
            compact_path(path, 56)
        ));
        aggregate.metadata_reads = 1;
        return Some(HistoryCell::exploration(
            "File info",
            "checked 1 path".to_string(),
            aggregate,
            HistoryTone::Control,
        ));
    }

    Some(HistoryCell::info(
        humanize_tool_label(tool_name),
        format!("metadata missing 鈥?{}", compact_path(path, 56)),
        HistoryTone::Warning,
    ))
}

fn render_web_search_result(structured: Option<&StructuredToolResult>) -> Option<HistoryCell> {
    let Some(StructuredToolResult::WebSearch { query, action, .. }) = structured else {
        return None;
    };

    Some(build_web_search_cell(query, action.as_ref(), None, None))
}

fn render_tool_error_result(
    tool_name: &str,
    structured: Option<&StructuredToolResult>,
) -> Option<HistoryCell> {
    let Some(StructuredToolResult::ToolError { message, .. }) = structured else {
        return None;
    };

    Some(HistoryCell::info(
        humanize_tool_label(tool_name),
        compact_inline(message, 100),
        HistoryTone::Error,
    ))
}

fn build_web_search_cell(
    query: &str,
    action: Option<&WebSearchAction>,
    source_count: Option<usize>,
    result_count: Option<usize>,
) -> HistoryCell {
    let presentation = web_search_card_presentation(query, action, source_count, result_count);
    let mut fields = Vec::new();
    if let Some(count) = source_count {
        fields.push(("sources", count.to_string()));
    }
    if let Some(count) = result_count {
        fields.push(("results", count.to_string()));
    }

    let detail = build_search_detail(presentation.detail, fields);
    HistoryCell::search("Web search", presentation.summary, Some(detail), HistoryTone::Control)
}

fn format_search_workspace_detail(
    query: &str,
    mode: SearchWorkspaceMode,
    status: SearchWorkspaceStatus,
    file_count: usize,
    match_count: usize,
    truncated: bool,
) -> String {
    let mode = match mode {
        SearchWorkspaceMode::Files => "files",
        SearchWorkspaceMode::Text => "text",
    };
    let status = match status {
        SearchWorkspaceStatus::Active => "active",
        SearchWorkspaceStatus::Closed => "closed",
        SearchWorkspaceStatus::NotFound => "missing",
    };
    let mut fields = vec![
        ("mode", mode.to_string()),
        ("files", file_count.to_string()),
        ("hits", match_count.to_string()),
        ("status", status.to_string()),
    ];
    if truncated {
        fields.push(("truncated", "yes".to_string()));
    }
    build_search_detail(compact_inline(query, 72), fields)
}

fn format_tool_search_detail(
    query: &str,
    match_count: usize,
    shown_count: usize,
    max_results: usize,
) -> String {
    build_search_detail(
        compact_inline(query, 72),
        vec![
            ("matched", match_count.to_string()),
            ("showing", shown_count.to_string()),
            ("max", max_results.to_string()),
        ],
    )
}

fn build_search_detail(query: String, fields: Vec<(&'static str, String)>) -> String {
    let mut lines = vec![format!("query: {query}")];
    lines.extend(
        fields
            .into_iter()
            .map(|(label, value)| format!("{label}: {value}")),
    );
    lines.join("\n")
}

fn search_status_suffix(status: &SearchWorkspaceStatus) -> &'static str {
    match status {
        SearchWorkspaceStatus::Active => "",
        SearchWorkspaceStatus::Closed => " closed",
        SearchWorkspaceStatus::NotFound => " missing",
    }
}

fn runtime_summary(item: &RuntimeItem) -> Option<String> {
    item.progress
        .as_ref()
        .and_then(|progress| progress.message.clone())
        .or_else(|| item.summary.clone())
        .filter(|summary| !summary.trim().is_empty())
}

fn first_non_empty_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("tool completed")
}
