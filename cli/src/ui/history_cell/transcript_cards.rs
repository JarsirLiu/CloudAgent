use super::display_common::tint_tail_style;
use super::wrapping::{WrapOptions, word_wrap_text};
use super::{HistoryCell, HistoryKind};
use crate::ui::theme::{history_body_style, history_title_accent_style, history_tool_style};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

pub(super) fn render_compact_transcript(
    cell: &HistoryCell,
    width: usize,
    bullet: &str,
) -> Vec<Line<'static>> {
    let title = if cell.label().is_empty() {
        match cell.kind() {
            HistoryKind::Exploration => "Explored workspace".to_string(),
            HistoryKind::Search => "Search".to_string(),
            HistoryKind::Command => "Command".to_string(),
            HistoryKind::Patch => "Patch".to_string(),
            HistoryKind::Tool => "Tool".to_string(),
            _ => "Step".to_string(),
        }
    } else {
        cell.label().to_string()
    };

    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{bullet} "), history_tool_style()),
        Span::styled(
            title,
            history_title_accent_style().add_modifier(Modifier::BOLD),
        ),
    ])];
    lines.extend(
        word_wrap_text(
            cell.body(),
            WrapOptions::new(width)
                .initial_indent(Line::from("  "))
                .subsequent_indent(Line::from("  ")),
        )
        .into_iter()
        .map(tint_tail_style(history_body_style())),
    );
    lines
}
