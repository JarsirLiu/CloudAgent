use std::cell::RefCell;

use agent_protocol::FrontendMode;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::input::completion::CompletionState;
use crate::text_width::display_width;
use crate::ui::widgets::completion_popup::completion_popup_lines;
use crate::ui::widgets::textarea::{TextArea, TextAreaState};

pub struct ComposerRender {
    pub lines: Vec<Line<'static>>,
    pub completion_lines: Vec<Line<'static>>,
    pub cursor_row: u16,
    pub height: u16,
}

pub(super) const MAX_VISIBLE_COMPOSER_ROWS: usize = 8;

struct ComposerLayout {
    prompt_prefix: String,
    prompt_width: usize,
    content_width: usize,
}

pub(super) fn render_composer(
    textarea: &TextArea,
    textarea_state: &RefCell<TextAreaState>,
    completion: &CompletionState,
    mode: FrontendMode,
    width: usize,
) -> ComposerRender {
    let (prompt_color, prompt_bg) = match mode {
        FrontendMode::WaitingForServerRequest => (Color::Rgb(255, 184, 76), None),
        FrontendMode::Running => (Color::Rgb(100, 160, 255), None),
        FrontendMode::Idle => (Color::Rgb(150, 180, 255), None),
    };
    let layout = composer_layout(mode, width);
    let body = composer_body(textarea, mode);

    let full_height = if textarea.is_empty() {
        textarea.wrapped_lines(body, layout.content_width).len() as u16
    } else {
        textarea.desired_height(layout.content_width)
    };
    let is_placeholder = textarea.is_empty();
    let mut lines = Vec::new();
    let visible_height = full_height.clamp(1, MAX_VISIBLE_COMPOSER_ROWS as u16);
    let (visible_lines, cursor_row, scroll_top) = if textarea.is_empty() {
        let wrapped = textarea.wrapped_lines(body, layout.content_width);
        let visible_height_usize = visible_height as usize;
        let scroll_top = wrapped.len().saturating_sub(visible_height_usize);
        let cursor_row = wrapped.len().saturating_sub(scroll_top).saturating_sub(1) as u16;
        (
            wrapped
                .into_iter()
                .skip(scroll_top)
                .take(visible_height_usize)
                .collect::<Vec<_>>(),
            cursor_row,
            scroll_top,
        )
    } else {
        let mut state = textarea_state.borrow_mut();
        let visible_lines =
            textarea.visible_wrapped_lines(body, layout.content_width, visible_height, &mut state);
        let (cursor_row, _) = textarea.visual_cursor_position_with_state(
            layout.content_width,
            visible_height,
            &mut state,
        );
        (visible_lines, cursor_row as u16, state.scroll as usize)
    };

    for (visible_index, wrapped_line) in visible_lines.into_iter().enumerate() {
        let actual_index = scroll_top + visible_index;
        let indent = if actual_index == 0 {
            layout.prompt_prefix.clone()
        } else {
            " ".repeat(layout.prompt_width)
        };
        let prompt_style = {
            let base = Style::default()
                .fg(prompt_color)
                .add_modifier(Modifier::BOLD);
            if actual_index == 0 {
                prompt_bg.map_or(base, |bg| base.bg(bg))
            } else {
                Style::default().fg(Color::Rgb(55, 55, 68))
            }
        };
        lines.push(Line::from(vec![
            Span::styled(indent, prompt_style),
            Span::styled(
                wrapped_line,
                if is_placeholder {
                    Style::default().fg(Color::Rgb(65, 65, 80))
                } else if textarea.is_all_selected() {
                    Style::default()
                        .fg(Color::Rgb(40, 40, 52))
                        .bg(Color::Rgb(220, 220, 230))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(220, 220, 230))
                },
            ),
        ]));
    }
    let completion_lines = completion_popup_lines(completion, width, layout.prompt_width);

    ComposerRender {
        lines,
        completion_lines,
        cursor_row,
        height: visible_height,
    }
}

pub(super) fn composer_desired_height(
    textarea: &TextArea,
    mode: FrontendMode,
    width: usize,
) -> u16 {
    let layout = composer_layout(mode, width);
    let body = composer_body(textarea, mode);
    textarea
        .wrapped_lines(body, layout.content_width)
        .len()
        .clamp(1, MAX_VISIBLE_COMPOSER_ROWS) as u16
}

pub(super) fn composer_cursor_position(
    textarea: &TextArea,
    textarea_state: &RefCell<TextAreaState>,
    area: Rect,
    mode: FrontendMode,
) -> (u16, u16) {
    let layout = composer_layout(mode, area.width as usize);
    let visible_height = composer_desired_height(textarea, mode, area.width as usize);
    let (cursor_row, cursor_col) = if textarea.is_empty() {
        (0, 0)
    } else {
        let mut state = textarea_state.borrow_mut();
        textarea.visual_cursor_position_with_state(layout.content_width, visible_height, &mut state)
    };
    let max_x_offset = area.width.saturating_sub(1) as usize;
    let x = area.x + (layout.prompt_width + cursor_col).min(max_x_offset) as u16;
    let y = area.y + cursor_row as u16;
    (x, y)
}

fn composer_layout(mode: FrontendMode, width: usize) -> ComposerLayout {
    let prompt_text = match mode {
        FrontendMode::WaitingForServerRequest => "?",
        FrontendMode::Running | FrontendMode::Idle => "›",
    };
    let prompt_prefix = format!("  {prompt_text} ");
    let prompt_width = display_width(&prompt_prefix);
    let content_width = width.saturating_sub(prompt_width + 2).max(10);
    ComposerLayout {
        prompt_prefix,
        prompt_width,
        content_width,
    }
}

fn composer_body(textarea: &TextArea, mode: FrontendMode) -> &str {
    if !textarea.is_empty() {
        return textarea.text();
    }

    match mode {
        FrontendMode::Idle => "Ask anything — e.g. \"check disk pressure\"",
        FrontendMode::WaitingForServerRequest => "Type y / n, or enter a short reason",
        FrontendMode::Running => "",
    }
}
