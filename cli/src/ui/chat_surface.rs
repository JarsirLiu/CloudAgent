use crate::app::TuiApp;
use crate::state::status_view_model::build_status_view_model;
use crate::terminal::Frame;
use crate::ui::chat_surface_model::{ChatSurfaceBody, ChatSurfaceModel, build_chat_surface_model};
use crate::ui::widgets::welcome::WelcomeScreen;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

const MAX_CONTENT_WIDTH: u16 = 140;
const MIN_RENDER_WIDTH: u16 = 40;
const HORIZONTAL_CHROME_WIDTH: u16 = 6;

pub(crate) struct ChatSurface;

pub(crate) struct ChatSurfaceLayout {
    pub(crate) render_width: usize,
    pub(crate) viewport_height: u16,
    content: Rect,
    body_area: Rect,
    bottom_area: Rect,
}

impl ChatSurface {
    pub(crate) fn render(app: &mut TuiApp, frame: &mut Frame) {
        let area = frame.area();
        let layout = compute_layout(app, area);
        let max_body_height = layout.body_area.height.saturating_sub(2).max(1) as usize;
        let surface_model = build_chat_surface_model(app, layout.render_width, max_body_height);
        let layout = apply_body_height(app, layout, &surface_model);

        render_body_area(app, frame, layout.body_area, surface_model);
        let status = build_status_view_model(app);
        let bottom = app.input_pane.render(
            frame,
            layout.bottom_area,
            app.console_state.mode,
            &status.text,
            &status.meta,
            &status.hint_meta,
        );

        if let Some((x, y)) = bottom.cursor_position {
            frame.set_cursor_position((x, y));
        }
    }

    pub(crate) fn desired_viewport_height(app: &mut TuiApp, terminal_area: Rect) -> u16 {
        let layout = compute_layout(app, terminal_area);
        let max_body_height = layout.body_area.height.saturating_sub(2).max(1) as usize;
        let surface_model = build_chat_surface_model(app, layout.render_width, max_body_height);
        let layout = apply_body_height(app, layout, &surface_model);
        layout.viewport_height.max(1)
    }
}

fn compute_layout(app: &TuiApp, area: Rect) -> ChatSurfaceLayout {
    let content = centered_column(area, MAX_CONTENT_WIDTH);
    let bottom_height = bottom_pane_height(app, content.width)
        .min(content.height)
        .max(1);
    let render_width = content
        .width
        .saturating_sub(HORIZONTAL_CHROME_WIDTH)
        .max(MIN_RENDER_WIDTH) as usize;
    let [body_area, bottom_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(bottom_height)]).areas(content);

    ChatSurfaceLayout {
        render_width,
        viewport_height: content.height.max(1),
        content,
        body_area,
        bottom_area,
    }
}

fn apply_body_height(
    app: &mut TuiApp,
    mut layout: ChatSurfaceLayout,
    surface_model: &ChatSurfaceModel,
) -> ChatSurfaceLayout {
    if matches!(surface_model.body, ChatSurfaceBody::Welcome) {
        app.transcript_state.clear_inline_viewport_height_lock();
        layout.viewport_height = layout.content.height.max(1);
        return layout;
    }

    let desired_body_height = surface_model.body_height.min(layout.body_area.height);
    let stack_height = desired_body_height
        .saturating_add(layout.bottom_area.height)
        .max(1);
    let desired_viewport_height = stack_height.min(layout.content.height.max(1));
    layout.viewport_height = resolved_viewport_height(app, desired_viewport_height);

    let top_spacer = layout.content.height.saturating_sub(stack_height);
    let [_, body_area, bottom_area] = Layout::vertical([
        Constraint::Length(top_spacer),
        Constraint::Length(desired_body_height),
        Constraint::Length(layout.bottom_area.height),
    ])
    .areas(layout.content);
    layout.body_area = body_area;
    layout.bottom_area = bottom_area;
    layout
}

fn resolved_viewport_height(app: &mut TuiApp, desired_height: u16) -> u16 {
    match app.console_state.mode {
        agent_protocol::FrontendMode::Running
        | agent_protocol::FrontendMode::WaitingForServerRequest => app
            .transcript_state
            .lock_inline_viewport_height(desired_height),
        _ => {
            app.transcript_state.clear_inline_viewport_height_lock();
            desired_height
        }
    }
}

fn bottom_pane_height(app: &TuiApp, width: u16) -> u16 {
    app.input_pane
        .desired_height(app.console_state.mode, width)
        .max(1)
}

fn render_body_area(app: &TuiApp, frame: &mut Frame, area: Rect, model: ChatSurfaceModel) {
    match model.body {
        ChatSurfaceBody::Welcome => render_welcome(app, frame, area),
        ChatSurfaceBody::ActiveCell(active_cell) => {
            render_active_cell(frame, area, active_cell.height, active_cell.lines)
        }
    }
}

fn render_active_cell(
    frame: &mut Frame,
    area: Rect,
    active_cell_height: u16,
    lines: Vec<Line<'static>>,
) {
    if area.height == 0 {
        return;
    }
    let active_area = active_cell_area(area, active_cell_height);
    let inner = active_area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
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

fn active_cell_area(area: Rect, active_cell_height: u16) -> Rect {
    let constrained_height = active_cell_height.min(area.height).max(1);
    let top_spacer = area.height.saturating_sub(constrained_height);
    let [_, active_area] = Layout::vertical([
        Constraint::Length(top_spacer),
        Constraint::Length(constrained_height),
    ])
    .areas(area);
    active_area
}
