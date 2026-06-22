use super::HistoryCell;
use super::exploration_cards;
use super::notice_cards;
use super::tool_cards;
use super::transcript_cards;
use ratatui::text::Line;

pub(crate) fn render_exploration(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    exploration_cards::render_exploration(cell, width)
}

pub(crate) fn render_search(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    super::search_cards::render_search(cell, width)
}

pub(crate) fn render_command(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    tool_cards::render_command(cell, width)
}

pub(crate) fn render_patch(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    tool_cards::render_patch(cell, width)
}

pub(crate) fn render_tool(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    tool_cards::render_tool(cell, width)
}

pub(crate) fn render_notice(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    notice_cards::render_notice(cell, width)
}

pub(crate) fn render_notice_transcript(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    notice_cards::render_notice_transcript(cell, width)
}

pub(crate) fn render_compact_transcript(
    cell: &HistoryCell,
    width: usize,
    bullet: &str,
) -> Vec<Line<'static>> {
    transcript_cards::render_compact_transcript(cell, width, bullet)
}
