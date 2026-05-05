use super::{ExplorationAggregate, HistoryCell, HistoryKind, HistoryTone};

pub(super) fn coalesce_tool_like(
    prev: &mut HistoryCell,
    next: &HistoryCell,
    allow_exploration: bool,
) -> bool {
    if allow_exploration
        && prev.kind() == HistoryKind::Exploration
        && next.kind() == HistoryKind::Exploration
    {
        return coalesce_exploration(prev, next);
    }
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

fn coalesce_exploration(prev: &mut HistoryCell, next: &HistoryCell) -> bool {
    let Some(prev_aggregate) = prev.aggregate().cloned() else {
        return false;
    };
    let Some(next_aggregate) = next.aggregate().cloned() else {
        return false;
    };

    let mut combined = ExplorationAggregate {
        read_files: prev_aggregate.read_files + next_aggregate.read_files,
        searches: prev_aggregate.searches + next_aggregate.searches,
        inspect_commands: prev_aggregate.inspect_commands + next_aggregate.inspect_commands,
        listed_directories: prev_aggregate.listed_directories + next_aggregate.listed_directories,
        metadata_reads: prev_aggregate.metadata_reads + next_aggregate.metadata_reads,
        details: prev_aggregate.details,
    };
    combined.details.extend(next_aggregate.details);

    prev.set_summary(format_exploration_summary(&combined));
    prev.set_aggregate(combined);
    prev.repeat_count = prev.repeat_count.saturating_add(next.repeat_count.max(1));
    true
}

fn format_exploration_summary(aggregate: &ExplorationAggregate) -> String {
    let mut parts = Vec::new();
    if aggregate.searches > 0 {
        parts.push(format!(
            "searched {} time{}",
            aggregate.searches,
            plural(aggregate.searches)
        ));
    }
    if aggregate.read_files > 0 {
        parts.push(format!(
            "read {} file{}",
            aggregate.read_files,
            plural(aggregate.read_files)
        ));
    }
    if aggregate.listed_directories > 0 {
        parts.push(format!(
            "listed {} director{}",
            aggregate.listed_directories,
            if aggregate.listed_directories == 1 {
                "y"
            } else {
                "ies"
            }
        ));
    }
    if aggregate.metadata_reads > 0 {
        parts.push(format!(
            "checked {} path{}",
            aggregate.metadata_reads,
            plural(aggregate.metadata_reads)
        ));
    }
    if aggregate.inspect_commands > 0 {
        parts.push(format!(
            "ran {} inspect command{}",
            aggregate.inspect_commands,
            plural(aggregate.inspect_commands)
        ));
    }
    if parts.is_empty() {
        "explored workspace".to_string()
    } else {
        parts.join(", ")
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn matches_tool_like(tone: HistoryTone) -> bool {
    matches!(
        tone,
        HistoryTone::Tool | HistoryTone::Control | HistoryTone::Warning | HistoryTone::Error
    )
}
