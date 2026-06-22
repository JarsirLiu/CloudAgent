use ratatui::style::{Color, Style};

use super::palette::*;

pub(crate) fn user_message_style() -> Style {
    Style::default().bg(Color::Rgb(
        HISTORY_USER_BG_RGB.0,
        HISTORY_USER_BG_RGB.1,
        HISTORY_USER_BG_RGB.2,
    ))
}

pub(crate) fn user_marker_style() -> Style {
    Style::default().fg(Color::Rgb(140, 150, 170))
}

pub(crate) fn history_body_style() -> Style {
    Style::default().fg(Color::Rgb(
        HISTORY_BODY_RGB.0,
        HISTORY_BODY_RGB.1,
        HISTORY_BODY_RGB.2,
    ))
}

pub(crate) fn history_rail_style() -> Style {
    Style::default().fg(Color::Rgb(
        HISTORY_RAIL_RGB.0,
        HISTORY_RAIL_RGB.1,
        HISTORY_RAIL_RGB.2,
    ))
}

pub(crate) fn history_note_style() -> Style {
    Style::default().fg(Color::Rgb(
        HISTORY_NOTE_RGB.0,
        HISTORY_NOTE_RGB.1,
        HISTORY_NOTE_RGB.2,
    ))
}

pub(crate) fn history_dim_style() -> Style {
    Style::default().fg(Color::Rgb(
        HISTORY_DIM_RGB.0,
        HISTORY_DIM_RGB.1,
        HISTORY_DIM_RGB.2,
    ))
}

pub(crate) fn history_title_style() -> Style {
    history_body_style().add_modifier(ratatui::style::Modifier::BOLD)
}

pub(crate) fn history_more_style() -> Style {
    history_note_style()
}

pub(crate) fn history_strong_text_style() -> Style {
    Style::default()
        .fg(Color::Rgb(
            HISTORY_BODY_RGB.0,
            HISTORY_BODY_RGB.1,
            HISTORY_BODY_RGB.2,
        ))
        .add_modifier(ratatui::style::Modifier::BOLD)
}

pub(crate) fn history_meta_marker_style() -> Style {
    Style::default().fg(Color::Rgb(80, 80, 90))
}

pub(crate) fn history_meta_style() -> Style {
    Style::default().fg(Color::Rgb(110, 110, 120))
}

pub(crate) fn history_title_accent_style() -> Style {
    Style::default().fg(Color::Rgb(215, 220, 232))
}

pub(crate) fn history_reasoning_style() -> Style {
    Style::default().fg(Color::Rgb(
        HISTORY_REASONING_RGB.0,
        HISTORY_REASONING_RGB.1,
        HISTORY_REASONING_RGB.2,
    ))
}

pub(crate) fn history_tool_style() -> Style {
    Style::default().fg(Color::Rgb(
        HISTORY_TOOL_RGB.0,
        HISTORY_TOOL_RGB.1,
        HISTORY_TOOL_RGB.2,
    ))
}

pub(crate) fn history_patch_style() -> Style {
    Style::default().fg(Color::Rgb(120, 200, 180))
}

pub(crate) fn history_search_style() -> Style {
    Style::default().fg(Color::Rgb(120, 170, 255))
}

pub(crate) fn history_notice_warning_style() -> Style {
    Style::default().fg(Color::Rgb(255, 196, 108))
}

pub(crate) fn history_notice_error_style() -> Style {
    Style::default().fg(Color::Rgb(255, 120, 120))
}

pub(crate) fn history_notice_control_style() -> Style {
    history_dim_style()
}
