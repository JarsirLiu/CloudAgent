use super::tool_common::{compact_inline, compact_path, format_line_range, humanize_tool_label};
use super::{ExplorationAggregate, HistoryCell, HistoryTone};
use crate::runtime_metrics_display::format_runtime_metrics;
use agent_core::{
    RuntimeItem, SearchWorkspaceMode, SearchWorkspaceStatus, StructuredToolResult, TurnItemKind,
    web_search_detail,
};

pub(super) fn render_active_placeholder(kind: TurnItemKind, title: &str) -> HistoryCell {
    match kind {
        TurnItemKind::ToolCall => HistoryCell::info(
            humanize_tool_label(title),
            "running".to_string(),
            HistoryTone::Control,
        ),
        TurnItemKind::ToolResult => HistoryCell::info(
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
        Some(StructuredToolResult::WebSearch { query, action, .. }) => {
            let detail = web_search_detail(query, action.as_ref());
            (!detail.trim().is_empty()).then_some(detail)
        }
        _ => item
            .progress
            .as_ref()
            .and_then(|progress| progress.message.clone())
            .or_else(|| item.summary.clone()),
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
    if let Some(StructuredToolResult::CommandExecution {
        command,
        current_directory,
        status,
        exit_code,
        output,
        ..
    }) = structured
    {
        return super::command::render_command_execution(
            tool_name,
            command,
            current_directory,
            status,
            *exit_code,
            output.as_deref(),
        );
    }
    if let Some(StructuredToolResult::ReadFile {
        path,
        total_chars,
        read,
        ..
    }) = structured
    {
        let display_path = compact_path(path, 56);
        let range_suffix = format_line_range(read.start_line, read.end_line);
        let detail = format!(
            "{}{} — {} chars{}",
            display_path,
            range_suffix,
            total_chars,
            if read.truncated { " truncated" } else { "" }
        );
        let mut aggregate = ExplorationAggregate::new(detail);
        aggregate.read_files = 1;
        return HistoryCell::exploration(
            "Read file",
            "read 1 file".to_string(),
            aggregate,
            HistoryTone::Control,
        );
    }
    if let Some(StructuredToolResult::SearchWorkspace {
        mode,
        status,
        query,
        file_count,
        match_count,
        truncated,
        ..
    }) = structured
    {
        let status_suffix = match status {
            SearchWorkspaceStatus::Active => "",
            SearchWorkspaceStatus::Closed => " closed",
            SearchWorkspaceStatus::NotFound => " missing",
        };
        let truncation = if *truncated { " truncated" } else { "" };
        let summary = match mode {
            SearchWorkspaceMode::Files => {
                format!("found {file_count} files{truncation}{status_suffix}")
            }
            SearchWorkspaceMode::Text => {
                format!(
                    "matched {match_count} hits in {file_count} files{truncation}{status_suffix}"
                )
            }
        };
        let detail = match mode {
            SearchWorkspaceMode::Files => {
                format!("file search `{}`", compact_inline(query, 48))
            }
            SearchWorkspaceMode::Text => {
                format!("text search `{}`", compact_inline(query, 48))
            }
        };
        let mut aggregate = ExplorationAggregate::new(detail);
        aggregate.searches = 1;
        return HistoryCell::exploration(
            "Search workspace",
            summary,
            aggregate,
            HistoryTone::Control,
        );
    }
    if let Some(StructuredToolResult::ToolSearch {
        query,
        max_results,
        match_count,
        hits,
    }) = structured
    {
        let mut aggregate =
            ExplorationAggregate::new(format!("tool search `{}`", compact_inline(query, 48)));
        aggregate.searches = 1;
        aggregate.push_detail(format!(
            "matched {match_count} tools, showing {} of {max_results}",
            hits.len()
        ));
        return HistoryCell::exploration(
            "Search tools",
            format!("matched {match_count} tools"),
            aggregate,
            HistoryTone::Control,
        );
    }
    if let Some(StructuredToolResult::ReadDirectory {
        path,
        entry_count,
        truncated,
        ..
    }) = structured
    {
        let mut aggregate = ExplorationAggregate::new(format!(
            "{} — {} entries{}",
            compact_path(path, 56),
            entry_count,
            if *truncated { " truncated" } else { "" }
        ));
        aggregate.listed_directories = 1;
        return HistoryCell::exploration(
            "Read directory",
            "listed 1 directory".to_string(),
            aggregate,
            HistoryTone::Control,
        );
    }
    if let Some(StructuredToolResult::GetMetadata {
        path,
        size,
        exists,
        is_file,
        ..
    }) = structured
    {
        if *exists {
            let kind = if *is_file { "file" } else { "directory" };
            let mut aggregate = ExplorationAggregate::new(format!(
                "metadata {} — {kind} ({size} bytes)",
                compact_path(path, 56)
            ));
            aggregate.metadata_reads = 1;
            return HistoryCell::exploration(
                "File info",
                "checked 1 path".to_string(),
                aggregate,
                HistoryTone::Control,
            );
        }
        return HistoryCell::info(
            humanize_tool_label(tool_name),
            format!("metadata missing — {}", compact_path(path, 56)),
            HistoryTone::Warning,
        );
    }
    if let Some(StructuredToolResult::WebSearch { query, action, .. }) = structured {
        let detail = web_search_detail(query, action.as_ref());
        if !detail.trim().is_empty() {
            let mut aggregate = ExplorationAggregate::new(detail);
            aggregate.searches = 1;
            return HistoryCell::exploration(
                "Web search",
                "searched the web".to_string(),
                aggregate,
                HistoryTone::Control,
            );
        }
    }
    if let Some(StructuredToolResult::ToolError { message, .. }) = structured {
        return HistoryCell::info(
            humanize_tool_label(tool_name),
            compact_inline(message, 100),
            HistoryTone::Error,
        );
    }

    let first = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("tool completed");
    HistoryCell::info(
        humanize_tool_label(tool_name),
        compact_inline(first, 100),
        HistoryTone::Control,
    )
}
