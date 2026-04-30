use crate::app::TuiApp;
use crate::state::selectors::status_text_from_mode;
use crate::ui::widgets::welcome::WelcomeScreen;
use agent_protocol::FrontendMode;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

pub(crate) fn render_app(app: &mut TuiApp, frame: &mut Frame) {
    let area = frame.area();
    let content = centered_column(area, 112);
    let bottom_height = app
        .input_pane
        .desired_height(app.console_state.mode, content.width)
        .clamp(6, content.height.saturating_sub(10).max(6));
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(bottom_height),
        ])
        .split(content);

    frame.render_widget(header_block(app), sections[0]);
    if app.transcript_state.transcript.is_empty() {
        render_welcome(app, frame, sections[1]);
    } else {
        app.transcript_state.viewport_height = sections[1].height.saturating_sub(0) as usize;
        app.transcript_state.viewport_width = sections[1].width.saturating_sub(4) as usize;
        app.clamp_transcript_scroll();
        frame.render_widget(transcript_panel(app, sections[1]), sections[1]);
    }

    let bottom = app.input_pane.render(
        frame,
        sections[2],
        app.console_state.mode,
        &current_status_text(app),
        &status_meta_text(app),
    );

    if let Some((x, y)) = bottom.cursor_position {
        frame.set_cursor_position((x, y));
    }
}

fn header_block(app: &TuiApp) -> Paragraph<'static> {
    let status = match app.console_state.mode {
        FrontendMode::Idle => ("ready", Color::Green),
        FrontendMode::Running => ("working", Color::Cyan),
        FrontendMode::WaitingForServerRequest => ("action", Color::Yellow),
    };

    let scroll_hint = if app.transcript_state.scroll > 0 {
        format!("scroll +{}", app.transcript_state.scroll)
    } else {
        "live".to_string()
    };
    let tool_text = app
        .run_state
        .last_tool_name
        .as_ref()
        .map(|tool| format!("tool {tool}"));

    let mut spans = vec![
        Span::styled(
            "── CloudAgent",
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("state {}", status.0),
            Style::default().fg(status.1).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{} msgs", app.run_state.last_message_count),
            Style::default().fg(Color::Rgb(130, 140, 160)),
        ),
        Span::raw("  "),
        Span::styled(
            app.connection_label.clone(),
            Style::default().fg(Color::Rgb(90, 110, 140)),
        ),
        Span::raw("  "),
        Span::styled(scroll_hint, Style::default().fg(Color::DarkGray)),
    ];
    if let Some(tool_text) = tool_text {
        let insert_at = spans.len().saturating_sub(2);
        spans.splice(
            insert_at..insert_at,
            [
                Span::raw("  "),
                Span::styled(tool_text, Style::default().fg(Color::Rgb(130, 140, 160))),
            ],
        );
    }

    Paragraph::new(Text::from(vec![Line::from(spans)]))
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

fn transcript_panel(app: &TuiApp, area: Rect) -> Paragraph<'static> {
    let inner = area.inner(Margin {
        vertical: 0,
        horizontal: 2,
    });
    let lines = app.transcript_state.transcript.render_lines_with_tail(
        inner.width as usize,
        inner.height as usize,
        app.transcript_state.scroll,
        app.transcript_state.active_cell.as_ref(),
    );
    Paragraph::new(Text::from(lines)).block(Block::default())
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
        let mut token_text = format!(
            "in {} out {} cached {}",
            format_tokens(usage.input_tokens),
            format_tokens(usage.output_tokens),
            format_tokens(usage.cached_input_tokens)
        );
        if let Some(total) = &app.run_state.total_turn_usage {
            token_text.push_str(&format!(" total {}", format_tokens(total.total_tokens)));
        }
        parts.push(token_text);
    } else if let Some(total) = &app.run_state.total_turn_usage {
        parts.push(format!("total {}", format_tokens(total.total_tokens)));
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
