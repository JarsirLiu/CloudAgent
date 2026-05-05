use super::{HistoryCell, HistoryTone};

pub(super) fn coalesce_tool_like(prev: &mut HistoryCell, next: &HistoryCell) -> bool {
    if !matches_tool_like(prev.tone) || prev.tone != next.tone {
        return false;
    }
    if prev.label != next.label
        || prev.format() != next.format()
        || prev.body() != next.body()
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
