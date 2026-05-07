use super::{HistoryCell, HistoryContent, HistoryKind, HistoryTone};

pub(super) fn coalesce_agent_stream(prev: &mut HistoryCell, next: &HistoryCell) -> bool {
    if prev.tone != HistoryTone::Agent || next.tone != HistoryTone::Agent {
        return false;
    }
    if prev.kind() != HistoryKind::Message || next.kind() != HistoryKind::Message {
        return false;
    }
    if prev.format() != next.format() || !next.is_stream_continuation() {
        return false;
    }

    prev.append_body(next.body());
    true
}

pub(super) fn coalesce_tool_like(prev: &mut HistoryCell, next: &HistoryCell) -> bool {
    if !matches_tool_like(prev.tone) || prev.tone != next.tone {
        return false;
    }
    if prev.label() != next.label() || prev.format() != next.format() || prev.body() != next.body()
    {
        return false;
    }
    prev.repeat_count = prev.repeat_count.saturating_add(next.repeat_count.max(1));
    prev.invalidate_cache();
    true
}

pub(super) fn group_adjacent_tool_results(prev: &mut HistoryCell, next: &HistoryCell) -> bool {
    if prev.tone != next.tone || !matches!(prev.tone, HistoryTone::Control | HistoryTone::Tool) {
        return false;
    }
    if prev.label() != next.label() || prev.kind() != HistoryKind::Tool || next.kind() != HistoryKind::Tool
    {
        return false;
    }

    match &mut prev.content {
        HistoryContent::ToolGroup(group) => {
            if group.label != next.label() {
                return false;
            }
            group.children.push(next.clone());
            group.summary = summarize_group(&group.label, group.children.len());
            prev.invalidate_cache();
            true
        }
        HistoryContent::Edit(_) => {
            let label = prev.label().to_string();
            let children = vec![prev.clone(), next.clone()];
            let expanded = prev.expanded;
            let repeat_count = prev.repeat_count;
            *prev = HistoryCell::tool_group(
                label.clone(),
                summarize_group(&label, children.len()),
                children,
                prev.tone,
            );
            prev.expanded = expanded;
            prev.repeat_count = repeat_count;
            true
        }
        _ => false,
    }
}

fn matches_tool_like(tone: HistoryTone) -> bool {
    matches!(
        tone,
        HistoryTone::Tool | HistoryTone::Control | HistoryTone::Warning | HistoryTone::Error
    )
}

fn summarize_group(label: &str, count: usize) -> String {
    match label {
        "Search workspace" => format!("searched workspace {count} times"),
        "Read file" => format!("read {count} files"),
        "Read directory" => format!("listed {count} directories"),
        "Write file" => format!("wrote {count} files"),
        "Run command" => format!("ran {count} commands"),
        other => format!("{other} {count} times"),
    }
}
