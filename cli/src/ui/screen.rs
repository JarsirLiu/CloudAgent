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

pub(crate) fn render_app(app: &mut TuiApp, frame: &mut Frame) {
    let area = frame.area();
    let content = centered_column(area, MAX_CONTENT_WIDTH);
    let bottom_height = app
        .input_pane
        .desired_height(app.console_state.mode, content.width)
        .min(content.height)
        .max(1);

    let input_area = if should_show_welcome(app) && content.height > bottom_height + 2 {
        let welcome_height = WELCOME_HEIGHT.min(content.height.saturating_sub(bottom_height));
        let [welcome_area, input_area] = Layout::vertical([
            Constraint::Length(welcome_height),
            Constraint::Min(bottom_height),
        ])
        .areas(content);
        render_welcome(app, frame, welcome_area);
        input_area
    } else if let Some(active_height) = active_cell_height(app, content.width)
        && content.height > bottom_height + 1
    {
        let active_height = active_height.min(content.height.saturating_sub(bottom_height));
        let [active_area, input_area] = Layout::vertical([
            Constraint::Length(active_height),
            Constraint::Min(bottom_height),
        ])
        .areas(content);
        render_active_cell(app, frame, active_area);
        input_area
    } else {
        content
    };

    let bottom = app.input_pane.render(
        frame,
        input_area,
        app.console_state.mode,
        &current_status_text(app),
        &status_meta_text(app),
    );

    if let Some((x, y)) = bottom.cursor_position {
        frame.set_cursor_position((x, y));
    }
}

pub(crate) fn desired_app_height(app: &TuiApp, width: u16) -> u16 {
    let content_width = width.min(MAX_CONTENT_WIDTH);
    let input_height = app
        .input_pane
        .desired_height(app.console_state.mode, content_width)
        .max(1);
    if should_show_welcome(app) {
        input_height.saturating_add(WELCOME_HEIGHT)
    } else if let Some(active_height) = active_cell_height(app, content_width) {
        input_height.saturating_add(active_height)
    } else {
        input_height
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
    let inner = area.inner(Margin {
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
    let mut parts = vec![format!("{} msgs", app.run_state.last_message_count)];
    if let Some(usage) = &app.run_state.last_turn_usage {
        parts.push(format!(
            "in {} out {} cached {} total {}",
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
    if let Some(tool) = &app.run_state.last_tool_name {
        parts.push(format!("tool {tool}"));
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
