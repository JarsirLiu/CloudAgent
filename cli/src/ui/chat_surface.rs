use crate::app::TuiApp;
use crate::state::status_view_model::build_status_view_model;
use crate::terminal::Frame;
use crate::ui::widgets::history_cell::HistoryCell;
use crate::ui::widgets::live_status_cell::render_live_status_line;
use crate::ui::widgets::welcome::WelcomeScreen;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

const MAX_CONTENT_WIDTH: u16 = 140;

pub(crate) struct ChatSurface;

impl ChatSurface {
    pub(crate) fn render(app: &mut TuiApp, frame: &mut Frame) {
        let area = frame.area();
        let content = centered_column(area, MAX_CONTENT_WIDTH);
        let bottom_height = bottom_pane_height(app, content.width).min(content.height);
        let transcript_height = content.height.saturating_sub(bottom_height).max(1);

        let [transcript_area, bottom_area] = Layout::vertical([
            Constraint::Length(transcript_height),
            Constraint::Length(bottom_height.max(1)),
        ])
        .areas(content);

        render_transcript_area(app, frame, transcript_area);
        let status = build_status_view_model(app);
        let bottom = app.input_pane.render(
            frame,
            bottom_area,
            app.console_state.mode,
            &status.text,
            &status.meta,
            &status.hint_meta,
        );

        if let Some((x, y)) = bottom.cursor_position {
            frame.set_cursor_position((x, y));
        }
    }
}

fn bottom_pane_height(app: &TuiApp, width: u16) -> u16 {
    app.input_pane
        .desired_height(app.console_state.mode, width)
        .max(1)
}

fn render_transcript_area(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    if should_show_welcome(app) {
        render_welcome(app, frame, area);
        return;
    }

    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let render_width = inner.width.saturating_sub(2).max(40) as usize;
    let lines = visible_transcript_lines(app, render_width, inner.height as usize);
    frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }), inner);
}

fn visible_transcript_lines(
    app: &mut TuiApp,
    render_width: usize,
    max_lines: usize,
) -> Vec<Line<'static>> {
    let lines = transcript_lines(app, render_width as usize);
    app.transcript_state.note_total_lines(lines.len());
    app.transcript_state.set_viewport_height(max_lines);
    app.transcript_state.clamp_scroll();
    if lines.len() > max_lines {
        let offset = app.transcript_state.scroll_offset_lines;
        let start = lines
            .len()
            .saturating_sub(max_lines)
            .saturating_sub(offset);
        lines[start..start + max_lines].to_vec()
    } else {
        app.transcript_state.jump_to_bottom();
        lines
    }
}

fn transcript_lines(app: &TuiApp, render_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, cell) in app.history_cells().iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }
        push_cell_lines(&mut lines, cell, render_width);
    }
    if let Some(line) = render_live_status_line(app) {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines.push(line);
    }
    lines
}

fn push_cell_lines(lines: &mut Vec<Line<'static>>, cell: &HistoryCell, render_width: usize) {
    if !cell.body().trim().is_empty() {
        lines.extend(cell.to_lines_with_mode(render_width));
    }
}

fn render_welcome(app: &TuiApp, frame: &mut Frame, area: Rect) {
    let outer = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
        .split(outer);

    let left_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightRed));
    let right_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightRed));

    let left_inner = left_block.inner(cols[0]);
    let right_inner = right_block.inner(cols[1]);

    frame.render_widget(left_block, cols[0]);
    frame.render_widget(right_block, cols[1]);

    let tips = vec![
        Line::from(Span::styled(
            "Tips for getting started",
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Use /config to set your OpenAI-compatible API key, base URL, and model.",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "About this project",
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "CloudAgent is a terminal coding and ops assistant for your workspace.",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Try asking:",
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "scan this repo and summarize the architecture",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "run tests and explain failures with fix suggestions",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "add a new slash command and wire it end-to-end",
            Style::default().fg(Color::Gray),
        )),
    ];

    frame.render_widget(
        WelcomeScreen::new(
            app.run_state.history_loaded,
            build_status_view_model(app).text,
            app.welcome_animation_frame,
        )
        .render(left_inner),
        left_inner,
    );
    frame.render_widget(
        Paragraph::new(Text::from(tips)).wrap(Wrap { trim: false }),
        right_inner,
    );
}

fn should_show_welcome(app: &TuiApp) -> bool {
    app.transcript_state.transcript.is_empty()
        && app.run_state.history_loaded
        && render_live_status_line(app).is_none()
}

fn centered_column(area: Rect, max_width: u16) -> Rect {
    let width = area.width.min(max_width);
    let horizontal_padding = area.width.saturating_sub(width) / 2;
    Rect {
        x: area.x + horizontal_padding,
        y: area.y,
        width,
        height: area.height,
    }
}
