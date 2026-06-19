use agent_protocol::FrontendMode;
use ratatui::style::{Color, Modifier, Style};

use super::palette::*;

pub(crate) fn status_divider_style() -> Style {
    Style::default().fg(Color::Rgb(
        SURFACE_DIVIDER_RGB.0,
        SURFACE_DIVIDER_RGB.1,
        SURFACE_DIVIDER_RGB.2,
    ))
}

pub(crate) fn status_ready_style() -> Style {
    Style::default()
        .fg(Color::Rgb(
            ACCENT_GREEN_RGB.0,
            ACCENT_GREEN_RGB.1,
            ACCENT_GREEN_RGB.2,
        ))
        .bg(Color::Rgb(
            SURFACE_BG_GREEN_RGB.0,
            SURFACE_BG_GREEN_RGB.1,
            SURFACE_BG_GREEN_RGB.2,
        ))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn status_running_style() -> Style {
    Style::default()
        .fg(Color::Rgb(
            ACCENT_BLUE_RGB.0,
            ACCENT_BLUE_RGB.1,
            ACCENT_BLUE_RGB.2,
        ))
        .bg(Color::Rgb(
            SURFACE_BG_BLUE_RGB.0,
            SURFACE_BG_BLUE_RGB.1,
            SURFACE_BG_BLUE_RGB.2,
        ))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn status_request_style() -> Style {
    Style::default()
        .fg(Color::Rgb(
            ACCENT_AMBER_RGB.0,
            ACCENT_AMBER_RGB.1,
            ACCENT_AMBER_RGB.2,
        ))
        .bg(Color::Rgb(
            SURFACE_BG_AMBER_RGB.0,
            SURFACE_BG_AMBER_RGB.1,
            SURFACE_BG_AMBER_RGB.2,
        ))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn status_mode_style(mode: FrontendMode) -> Style {
    match mode {
        FrontendMode::Idle => status_ready_style(),
        FrontendMode::Running => status_running_style(),
        FrontendMode::WaitingForServerRequest => status_request_style(),
    }
}
