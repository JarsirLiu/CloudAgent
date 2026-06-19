use agent_protocol::FrontendMode;
use ratatui::style::{Color, Style};

use super::surface::*;

pub(crate) fn input_border_style(mode: FrontendMode) -> Style {
    match mode {
        FrontendMode::Idle => Style::default().fg(Color::Rgb(75, 82, 110)),
        FrontendMode::Running => Style::default().fg(Color::Rgb(82, 130, 190)),
        FrontendMode::WaitingForServerRequest => Style::default().fg(Color::Rgb(210, 150, 45)),
    }
}

pub(crate) fn input_title_style() -> Style {
    title_style()
}

pub(crate) fn input_completion_border_style() -> Style {
    border_style()
}

pub(crate) fn composer_prompt_style(mode: FrontendMode) -> Style {
    match mode {
        FrontendMode::WaitingForServerRequest => Style::default().fg(Color::Rgb(255, 184, 76)),
        FrontendMode::Running => Style::default().fg(Color::Rgb(100, 160, 255)),
        FrontendMode::Idle => Style::default().fg(Color::Rgb(150, 180, 255)),
    }
}

pub(crate) fn composer_prompt_faint_style() -> Style {
    Style::default().fg(Color::Rgb(55, 55, 68))
}

pub(crate) fn composer_body_placeholder_style() -> Style {
    Style::default().fg(Color::Rgb(65, 65, 80))
}

pub(crate) fn composer_body_style() -> Style {
    Style::default().fg(Color::Rgb(220, 220, 230))
}

pub(crate) fn composer_body_selected_style() -> Style {
    Style::default()
        .fg(Color::Rgb(40, 40, 52))
        .bg(Color::Rgb(220, 220, 230))
        .add_modifier(ratatui::style::Modifier::BOLD)
}
