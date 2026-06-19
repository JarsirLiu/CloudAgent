use ratatui::style::{Color, Modifier, Style};

use super::palette::*;
use super::surface::*;

pub(crate) fn picker_selected_style() -> Style {
    selected_row_style()
}

pub(crate) fn picker_unselected_style() -> Style {
    unselected_style()
}

pub(crate) fn picker_selected_alt_style() -> Style {
    Style::default()
        .fg(Color::Rgb(
            ACCENT_INFO_RGB.0,
            ACCENT_INFO_RGB.1,
            ACCENT_INFO_RGB.2,
        ))
        .bg(Color::Rgb(
            SURFACE_BG_SELECTED_RGB.0,
            SURFACE_BG_SELECTED_RGB.1,
            SURFACE_BG_SELECTED_RGB.2,
        ))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn picker_current_style() -> Style {
    Style::default().fg(Color::Rgb(
        ACCENT_INFO_RGB.0,
        ACCENT_INFO_RGB.1,
        ACCENT_INFO_RGB.2,
    ))
}

pub(crate) fn picker_meta_style() -> Style {
    unselected_style().fg(Color::Rgb(
        SURFACE_TEXT_SOFT_RGB.0,
        SURFACE_TEXT_SOFT_RGB.1,
        SURFACE_TEXT_SOFT_RGB.2,
    ))
}
