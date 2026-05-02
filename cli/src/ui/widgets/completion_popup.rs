use crate::input::completion::CompletionState;
use crate::ui::widgets::textarea::display_width;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const MAX_ROWS: usize = 6;
const COMMAND_COLUMN_WIDTH: usize = 14;

pub(crate) fn completion_popup_lines(
    completion: &CompletionState,
    width: usize,
    content_indent: usize,
) -> Vec<Line<'static>> {
    if !completion.is_active() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let (window_start, suggestions) = completion.visible_window(MAX_ROWS);
    let _has_more_above = window_start > 0;
    let _has_more_below = window_start + suggestions.len() < completion.suggestions().len();

    for (offset, suggestion) in suggestions.iter().enumerate() {
        let index = window_start + offset;
        let selected = index == completion.selected_index();
        let label = if suggestion.command.is_some() {
            format!("/{}", suggestion.name)
        } else {
            suggestion.name.to_string()
        };
        let name = format!("{:<width$}", label, width = COMMAND_COLUMN_WIDTH);
        let marker = if selected {
            "> "
        } else {
            "  "
        };
        let row_indent = content_indent.saturating_sub(marker.len());
        let description_width =
            width.saturating_sub(row_indent + marker.len() + COMMAND_COLUMN_WIDTH + 3);
        let description = truncate_to_width(suggestion.description, description_width);
        let row_width = row_indent + marker.len() + COMMAND_COLUMN_WIDTH + 2 + description_width;
        let row_text_width = row_indent
            + marker.len()
            + display_width(name.as_str())
            + 2
            + display_width(&description);
        let trailing_padding = " ".repeat(row_width.saturating_sub(row_text_width));

        let (marker_style, name_style, description_style, padding_style) = if selected {
            let bg = Color::Rgb(26, 34, 50);
            (
                Style::default()
                    .fg(Color::Rgb(120, 255, 170))
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
                Style::default()
                    .fg(Color::Rgb(190, 220, 255))
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(Color::Rgb(165, 170, 195)).bg(bg),
                Style::default().bg(bg),
            )
        } else {
            (
                Style::default().fg(Color::Rgb(75, 84, 105)),
                Style::default().fg(Color::Rgb(135, 145, 175)),
                Style::default().fg(Color::Rgb(95, 100, 124)),
                Style::default(),
            )
        };

        let indent_style = if selected {
            padding_style
        } else {
            Style::default()
        };

        lines.push(Line::from(vec![
            Span::styled(" ".repeat(row_indent), indent_style),
            Span::styled(marker.to_string(), marker_style),
            Span::styled(name, name_style),
            Span::styled("  ", padding_style),
            Span::styled(description, description_style),
            Span::styled(trailing_padding, padding_style),
        ]));
    }
    lines
}

fn truncate_to_width(value: &str, width: usize) -> String {
    if width == 0 || display_width(value) <= width {
        return value.to_string();
    }
    let mut out = String::new();
    for ch in value.chars() {
        let ch_text = ch.to_string();
        if display_width(out.as_str()) + display_width(ch_text.as_str()) + 3 > width {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}
