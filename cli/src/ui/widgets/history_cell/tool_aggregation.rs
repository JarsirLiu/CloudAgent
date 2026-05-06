use super::{HistoryCell, HistoryKind, HistoryTone};

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

fn matches_tool_like(tone: HistoryTone) -> bool {
    matches!(
        tone,
        HistoryTone::Tool | HistoryTone::Control | HistoryTone::Warning | HistoryTone::Error
    )
}
