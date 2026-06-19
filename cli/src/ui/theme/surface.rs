use ratatui::style::{Color, Modifier, Style};

use super::palette::*;

pub(crate) fn border_style() -> Style {
    Style::default().fg(Color::Rgb(
        SURFACE_BORDER_RGB.0,
        SURFACE_BORDER_RGB.1,
        SURFACE_BORDER_RGB.2,
    ))
}

pub(crate) fn title_style() -> Style {
    Style::default()
        .fg(Color::Rgb(
            SURFACE_TEXT_RGB.0,
            SURFACE_TEXT_RGB.1,
            SURFACE_TEXT_RGB.2,
        ))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn body_style() -> Style {
    Style::default().fg(Color::Rgb(
        SURFACE_TEXT_RGB.0,
        SURFACE_TEXT_RGB.1,
        SURFACE_TEXT_RGB.2,
    ))
}

pub(crate) fn muted_style() -> Style {
    Style::default().fg(Color::Rgb(
        SURFACE_TEXT_DIM_RGB.0,
        SURFACE_TEXT_DIM_RGB.1,
        SURFACE_TEXT_DIM_RGB.2,
    ))
}

pub(crate) fn hint_style() -> Style {
    Style::default().fg(Color::Rgb(
        SURFACE_TEXT_FAINT_RGB.0,
        SURFACE_TEXT_FAINT_RGB.1,
        SURFACE_TEXT_FAINT_RGB.2,
    ))
}

pub(crate) fn selected_style() -> Style {
    Style::default().fg(Color::Rgb(
        SURFACE_TEXT_STRONG_RGB.0,
        SURFACE_TEXT_STRONG_RGB.1,
        SURFACE_TEXT_STRONG_RGB.2,
    ))
}

pub(crate) fn selected_alt_style() -> Style {
    selected_style()
}

pub(crate) fn unselected_style() -> Style {
    Style::default().fg(Color::Rgb(
        SURFACE_TEXT_MID_RGB.0,
        SURFACE_TEXT_MID_RGB.1,
        SURFACE_TEXT_MID_RGB.2,
    ))
}

pub(crate) fn selected_row_style() -> Style {
    selected_style()
        .bg(Color::Rgb(
            SURFACE_BG_SELECTED_RGB.0,
            SURFACE_BG_SELECTED_RGB.1,
            SURFACE_BG_SELECTED_RGB.2,
        ))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn input_panel_bg() -> Color {
    Color::Rgb(
        SURFACE_BG_INPUT_RGB.0,
        SURFACE_BG_INPUT_RGB.1,
        SURFACE_BG_INPUT_RGB.2,
    )
}

pub(crate) fn info_style() -> Style {
    Style::default().fg(Color::Rgb(
        ACCENT_INFO_RGB.0,
        ACCENT_INFO_RGB.1,
        ACCENT_INFO_RGB.2,
    ))
}

pub(crate) fn disabled_style() -> Style {
    Style::default().fg(Color::Rgb(
        SURFACE_TEXT_MUTED_RGB.0,
        SURFACE_TEXT_MUTED_RGB.1,
        SURFACE_TEXT_MUTED_RGB.2,
    ))
}
