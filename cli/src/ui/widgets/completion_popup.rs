use crate::input::completion::CompletionState;
use crate::ui::widgets::textarea::display_width;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const MAX_ROWS: usize = 6;
const LEFT_COLUMN_WIDTH: usize = 14;

pub(crate) fn completion_popup_lines(
    completion: &CompletionState,
    width: usize,
) -> Vec<Line<'static>> {
    if !completion.is_active() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push(Line::raw(""));
    for (index, suggestion) in completion.suggestions().iter().take(MAX_ROWS).enumerate() {
        let selected = index == completion.selected_index();
        let name = format!(
            "/{:<width$}",
            suggestion.name,
            width = LEFT_COLUMN_WIDTH - 1
        );
        let description_width = width.saturating_sub(LEFT_COLUMN_WIDTH + 6);
        let description = truncate_to_width(suggestion.description, description_width);
        let marker = if selected { "  > " } else { "    " };
        let name_style = if selected {
            Style::default()
                .fg(Color::Rgb(145, 185, 255))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(120, 124, 148))
        };
        lines.push(Line::from(vec![
            Span::styled(marker.to_string(), name_style),
            Span::styled(name, name_style),
            Span::styled(description, Style::default().fg(Color::Rgb(120, 124, 148))),
        ]));
    }
    lines.push(Line::raw(""));
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
