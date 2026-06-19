use ratatui::style::{Color, Modifier, Style};

pub(crate) fn welcome_accent_style() -> Style {
    Style::default()
        .fg(Color::Rgb(140, 160, 230))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn welcome_mascot_color() -> Color {
    Color::Rgb(120, 130, 200)
}

pub(crate) fn welcome_mascot_style() -> Style {
    Style::default().fg(welcome_mascot_color())
}

pub(crate) fn welcome_signal_style() -> Style {
    Style::default().fg(Color::Rgb(100, 255, 100))
}
