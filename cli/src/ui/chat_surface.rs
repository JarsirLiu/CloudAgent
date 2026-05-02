use crate::app::TuiApp;
use crate::state::selectors::status_text_from_mode;
use crate::terminal::Frame;
use crate::ui::widgets::welcome::WelcomeScreen;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

const WELCOME_HEIGHT: u16 = 27;
const MAX_CONTENT_WIDTH: u16 = 140;
const ACTIVE_TOP_INSET: u16 = 1;

pub(crate) struct ChatSurface;

impl ChatSurface {
    pub(crate) fn render(app: &mut TuiApp, frame: &mut Frame) {
        let area = frame.area();
        let content = centered_column(area, MAX_CONTENT_WIDTH);
        let bottom_height = bottom_pane_height(app, content.width).min(content.height);
        let live_height = live_area_height(app, content.width)
            .unwrap_or(0)
            .min(content.height.saturating_sub(bottom_height));

        let (live_area, bottom_area) = split_live_and_bottom(content, live_height, bottom_height);
        render_live_area(app, frame, live_area);

        let bottom = app.input_pane.render(
            frame,
            bottom_area,
            app.console_state.mode,
            &current_status_text(app),
            &status_meta_text(app),
        );

        if let Some((x, y)) = bottom.cursor_position {
            frame.set_cursor_position((x, y));
        }
    }

    pub(crate) fn desired_height(app: &TuiApp, width: u16) -> u16 {
        let content_width = width.min(MAX_CONTENT_WIDTH);
        let bottom_height = bottom_pane_height(app, content_width);
        let live_height = live_area_height(app, content_width).unwrap_or(0);
        bottom_height.saturating_add(live_height).max(1)
    }
}

fn bottom_pane_height(app: &TuiApp, width: u16) -> u16 {
    app.input_pane
        .desired_height(app.console_state.mode, width)
        .max(1)
}

fn live_area_height(app: &TuiApp, width: u16) -> Option<u16> {
    if should_show_welcome(app) {
        Some(WELCOME_HEIGHT)
    } else {
        active_cell_height(app, width).map(|height| height.saturating_add(ACTIVE_TOP_INSET))
    }
}

fn split_live_and_bottom(content: Rect, live_height: u16, bottom_height: u16) -> (Rect, Rect) {
    if live_height == 0 {
        return (
            Rect {
                height: 0,
                ..content
            },
            Rect {
                height: bottom_height.max(1),
                ..content
            },
        );
    }

    let [live_area, bottom_area] = Layout::vertical([
        Constraint::Length(live_height),
        Constraint::Length(bottom_height.max(1)),
    ])
    .areas(content);
    (live_area, bottom_area)
}

fn render_live_area(app: &TuiApp, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    if should_show_welcome(app) {
        render_welcome(app, frame, area);
    } else {
        render_active_cell(app, frame, area);
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

    let recent = recent_activity_lines(app);
    let mut tips = vec![
        Line::from(Span::styled(
            "Tips for getting started",
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Run /init when you want a local AGENTS guide.",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Recent activity",
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    tips.extend(recent);
    tips.push(Line::from(""));
    tips.push(Line::from(Span::styled(
        "Try asking:",
        Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD),
    )));
    tips.push(Line::from(Span::styled(
        "check disk pressure",
        Style::default().fg(Color::Gray),
    )));
    tips.push(Line::from(Span::styled(
        "inspect this repo and explain it",
        Style::default().fg(Color::Gray),
    )));
    tips.push(Line::from(Span::styled(
        "write a safe nginx restart script",
        Style::default().fg(Color::Gray),
    )));

    frame.render_widget(
        WelcomeScreen::new(
            app.run_state.history_loaded,
            current_status_text(app),
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
    app.transcript_state.transcript.is_empty() && app.run_state.history_loaded
}

fn active_cell_height(app: &TuiApp, width: u16) -> Option<u16> {
    let active = app.transcript_state.active_cell.as_ref()?;
    if active.body.trim().is_empty() {
        return None;
    }
    let render_width = width.saturating_sub(4).max(40) as usize;
    Some(active.to_lines_with_mode(render_width).len().max(1) as u16)
}

fn render_active_cell(app: &TuiApp, frame: &mut Frame, area: Rect) {
    let Some(active) = app.transcript_state.active_cell.as_ref() else {
        return;
    };
    if active.body.trim().is_empty() {
        return;
    }
    let live_area = Rect {
        y: area.y.saturating_add(ACTIVE_TOP_INSET),
        height: area.height.saturating_sub(ACTIVE_TOP_INSET),
        ..area
    };
    let inner = live_area.inner(Margin {
        horizontal: 2,
        vertical: 0,
    });
    let render_width = inner.width.max(40) as usize;
    let mut lines = active.to_lines_with_mode(render_width);
    let max_lines = inner.height as usize;
    if lines.len() > max_lines {
        lines = lines[lines.len() - max_lines..].to_vec();
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn recent_activity_lines(app: &TuiApp) -> Vec<Line<'static>> {
    if app.transcript_state.transcript.is_empty() {
        return vec![Line::from(Span::styled(
            "No recent activity",
            Style::default().fg(Color::Gray),
        ))];
    }

    vec![
        Line::from(Span::styled(
            "Conversation has recent history",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "Use F2 to inspect transcript history",
            Style::default().fg(Color::DarkGray),
        )),
    ]
}

fn status_meta_text(app: &TuiApp) -> String {
    let mut parts = Vec::new();
    if let Some(usage) = &app.run_state.last_turn_usage {
        parts.push(format!(
            "in {} · out {} · cached {} · total {}",
            format_tokens(usage.input_tokens),
            format_tokens(usage.output_tokens),
            format_tokens(usage.cached_input_tokens),
            format_tokens(usage.total_tokens)
        ));
    }
    if let (Some(total), Some(window)) = (
        &app.run_state.total_turn_usage,
        app.run_state.model_context_window,
    ) && window > 0
    {
        let percent = total.total_tokens.saturating_mul(100) / window;
        parts.push(format!("context {percent}%"));
    }
    if let Some(activity) = &app.run_state.current_tool_activity {
        parts.push(activity.clone());
    }
    parts.join(" · ")
}

fn compact_number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn format_tokens(value: u64) -> String {
    format!("{} tokens", compact_number(value))
}

fn current_status_text(app: &TuiApp) -> String {
    if let Some(notice) = &app.run_state.status_notice {
        return notice.clone();
    }
    status_text_from_mode(app.console_state.mode).to_string()
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
