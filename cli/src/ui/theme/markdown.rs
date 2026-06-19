use ratatui::style::{Color, Style};

pub(crate) fn markdown_text_style() -> Style {
    Style::default().fg(Color::Rgb(200, 200, 210))
}

pub(crate) fn markdown_code_style() -> Style {
    Style::default().fg(Color::Rgb(210, 210, 220))
}

pub(crate) fn markdown_html_style() -> Style {
    Style::default().fg(Color::Rgb(140, 150, 170))
}

pub(crate) fn markdown_list_marker_style() -> Style {
    Style::default().fg(Color::Rgb(150, 180, 255))
}

pub(crate) fn markdown_table_separator_style() -> Style {
    Style::default().fg(Color::Rgb(90, 96, 108))
}
