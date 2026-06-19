use ratatui::style::{Color, Modifier, Style};

use super::palette::*;

pub(crate) fn request_title_style() -> Style {
    Style::default()
        .fg(Color::Rgb(
            ACCENT_AMBER_RGB.0,
            ACCENT_AMBER_RGB.1,
            ACCENT_AMBER_RGB.2,
        ))
        .bg(Color::Rgb(
            SURFACE_BG_TITLE_RGB.0,
            SURFACE_BG_TITLE_RGB.1,
            SURFACE_BG_TITLE_RGB.2,
        ))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn request_command_style() -> Style {
    Style::default().fg(Color::Rgb(
        SURFACE_TEXT_RGB.0,
        SURFACE_TEXT_RGB.1,
        SURFACE_TEXT_RGB.2,
    ))
}

pub(crate) fn request_muted_style() -> Style {
    Style::default().fg(Color::Rgb(
        SURFACE_TEXT_MUTED_RGB.0,
        SURFACE_TEXT_MUTED_RGB.1,
        SURFACE_TEXT_MUTED_RGB.2,
    ))
}

pub(crate) fn request_option_bg() -> Color {
    Color::Rgb(
        SURFACE_BG_INPUT_RGB.0,
        SURFACE_BG_INPUT_RGB.1,
        SURFACE_BG_INPUT_RGB.2,
    )
}

pub(crate) fn request_success_style() -> Style {
    Style::default().fg(Color::Rgb(
        ACCENT_SUCCESS_RGB.0,
        ACCENT_SUCCESS_RGB.1,
        ACCENT_SUCCESS_RGB.2,
    ))
}

pub(crate) fn request_cyan_style() -> Style {
    Style::default().fg(Color::Rgb(
        ACCENT_CYAN_RGB.0,
        ACCENT_CYAN_RGB.1,
        ACCENT_CYAN_RGB.2,
    ))
}

pub(crate) fn request_error_style() -> Style {
    Style::default().fg(Color::Rgb(
        ACCENT_RED_RGB.0,
        ACCENT_RED_RGB.1,
        ACCENT_RED_RGB.2,
    ))
}

pub(crate) fn request_amber_style() -> Style {
    Style::default().fg(Color::Rgb(
        ACCENT_AMBER_RGB.0,
        ACCENT_AMBER_RGB.1,
        ACCENT_AMBER_RGB.2,
    ))
}
