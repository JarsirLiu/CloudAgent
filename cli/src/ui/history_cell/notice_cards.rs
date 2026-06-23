use super::card_layout::{render_card_header, render_wrapped_body};
use super::display_common::tint_tail_style;
use super::wrapping::{WrapOptions, word_wrap_text};
use super::{HistoryCell, HistoryTone};
use crate::ui::theme::{
    history_body_style, history_meta_marker_style, history_meta_style,
    history_notice_control_style, history_notice_error_style, history_notice_warning_style,
    history_rail_style, history_title_accent_style, history_tool_style,
};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

pub(super) fn render_meta(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    word_wrap_text(
        cell.body(),
        WrapOptions::new(width)
            .initial_indent(Line::from(vec![Span::styled(
                "•",
                history_meta_marker_style(),
            )]))
            .subsequent_indent(Line::from("  ")),
    )
    .into_iter()
    .map(tint_tail_style(history_meta_style()))
    .collect()
}

pub(super) fn render_notice(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match cell.tone {
        HistoryTone::Warning => {
            render_notice_like(cell, width, history_notice_warning_style(), "◼")
        }
        HistoryTone::Error => render_notice_like(cell, width, history_notice_error_style(), "◼"),
        HistoryTone::Meta => render_meta(cell, width),
        _ => render_notice_like(cell, width, history_tool_style(), "▸"),
    }
}

pub(super) fn render_notice_transcript(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let accent = match cell.tone {
        HistoryTone::Error => history_notice_error_style(),
        HistoryTone::Warning => history_notice_warning_style(),
        HistoryTone::Control => history_notice_control_style(),
        _ => history_notice_control_style(),
    };
    let title = if cell.label().is_empty() {
        "Notice".to_string()
    } else {
        cell.label().to_string()
    };
    let mut lines = vec![render_card_header(
        "•",
        accent,
        title,
        history_title_accent_style(),
    )];
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

fn render_notice_like(
    cell: &HistoryCell,
    width: usize,
    accent: Style,
    dot: &str,
) -> Vec<Line<'static>> {
    let title = if cell.label().is_empty() {
        "Notice".to_string()
    } else {
        cell.label().to_string()
    };
    let mut lines = vec![render_card_header(
        dot,
        accent,
        title,
        history_title_accent_style(),
    )];
    lines.extend(render_wrapped_body(
        cell.body(),
        width,
        Line::from(vec![
            Span::raw("    "),
            Span::styled("└", history_rail_style()),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("└", history_rail_style()),
        ]),
        history_body_style(),
    ));
    lines
}
