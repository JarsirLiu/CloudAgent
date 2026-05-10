use crate::app::TuiApp;
use crate::app::runtime::display::should_show_welcome;
use crate::state::NoticeLevel;
use crate::terminal::Frame;
use crate::ui::chat_surface_model::{ChatSurfaceBody, ChatSurfaceModel, build_chat_surface_model};
use crate::ui::widgets::welcome::WelcomeScreen;
use agent_protocol::FrontendMode;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

const MAX_CONTENT_WIDTH: u16 = 140;
const MIN_RENDER_WIDTH: u16 = 40;
const HORIZONTAL_CHROME_WIDTH: u16 = 6;
const VIEWPORT_TOP_GUTTER_HEIGHT: u16 = 1;
const BODY_BOTTOM_GAP_HEIGHT: u16 = 1;

pub(crate) struct ChatSurface;

pub(crate) struct ChatSurfaceLayout {
    body_area: Rect,
    status_area: Rect,
    bottom_area: Rect,
}

impl ChatSurface {
    pub(crate) fn render_width_for_area(area: Rect) -> usize {
        let content = centered_column(area, MAX_CONTENT_WIDTH);
        content
            .width
            .saturating_sub(HORIZONTAL_CHROME_WIDTH)
            .max(MIN_RENDER_WIDTH) as usize
    }

    pub(crate) fn render(app: &mut TuiApp, frame: &mut Frame) {
        let area = frame.area();
        let mode = app.current_mode();
        let shows_welcome = matches!(mode, FrontendMode::Idle) && should_show_welcome(app);
        let surface_area = viewport_surface_area(area, shows_welcome);
        let status = app.bottom_pane.build_status_view_model(app);
        let content = centered_column(surface_area, MAX_CONTENT_WIDTH);
        let render_width = Self::render_width_for_area(surface_area);
        let bottom_height = bottom_pane_height(app, content.width)
            .min(content.height)
            .max(1);
        let status_height = if status.live_banner.is_some() { 1 } else { 0 };
        let max_body_height = available_body_height(
            content,
            bottom_height,
            status_height,
            BODY_BOTTOM_GAP_HEIGHT,
        ) as usize;
        let surface_model = build_chat_surface_model(app, render_width, max_body_height);
        let layout = compute_layout(
            app,
            surface_area,
            surface_model.body_height,
            status.live_banner.is_some(),
            matches!(surface_model.body, ChatSurfaceBody::Welcome),
        );

        render_body_area(app, frame, layout.body_area, surface_model);
        render_status_area(
            frame,
            layout.status_area,
            status.live_banner.as_deref(),
            status.live_banner_level,
        );
        let bottom = app.bottom_pane.render(
            frame,
            layout.bottom_area,
            mode,
            status.indicator.as_deref(),
            &status.text,
            status.runtime_hint.as_deref(),
            &status.meta,
            &status.hint_meta,
        );

        if let Some((x, y)) = bottom.cursor_position {
            frame.set_cursor_position((x, y));
        }
    }

    pub(crate) fn desired_viewport_height(app: &mut TuiApp, terminal_area: Rect) -> u16 {
        let mode = app.current_mode();
        let shows_welcome = matches!(mode, FrontendMode::Idle) && should_show_welcome(app);
        let surface_area = viewport_surface_area(terminal_area, shows_welcome);
        let status = app.bottom_pane.build_status_view_model(app);
        let content = centered_column(surface_area, MAX_CONTENT_WIDTH);
        let render_width = Self::render_width_for_area(surface_area);
        let bottom_height = bottom_pane_height(app, content.width)
            .min(content.height)
            .max(1);
        let status_height = if status.live_banner.is_some() { 1 } else { 0 };
        let max_body_height = available_body_height(
            content,
            bottom_height,
            status_height,
            BODY_BOTTOM_GAP_HEIGHT,
        ) as usize;
        let surface_model = build_chat_surface_model(app, render_width, max_body_height);
        desired_stack_height(
            app,
            surface_area,
            surface_model.body_height,
            status.live_banner.is_some(),
            matches!(surface_model.body, ChatSurfaceBody::Welcome),
        )
        .saturating_add(if shows_welcome {
            0
        } else {
            VIEWPORT_TOP_GUTTER_HEIGHT
        })
        .max(1)
    }
}

fn viewport_surface_area(area: Rect, is_welcome: bool) -> Rect {
    if is_welcome || area.height <= VIEWPORT_TOP_GUTTER_HEIGHT {
        area
    } else {
        Rect::new(
            area.x,
            area.y.saturating_add(VIEWPORT_TOP_GUTTER_HEIGHT),
            area.width,
            area.height.saturating_sub(VIEWPORT_TOP_GUTTER_HEIGHT),
        )
    }
}

fn compute_layout(
    app: &TuiApp,
    area: Rect,
    body_height: u16,
    has_status_banner: bool,
    is_welcome: bool,
) -> ChatSurfaceLayout {
    let content = centered_column(area, MAX_CONTENT_WIDTH);
    let bottom_height = bottom_pane_height(app, content.width)
        .min(content.height)
        .max(1);

    if is_welcome {
        let status_height = if has_status_banner { 1 } else { 0 };
        let [body_area, status_area, bottom_area] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(status_height),
            Constraint::Length(bottom_height),
        ])
        .areas(content);

        return ChatSurfaceLayout {
            body_area,
            status_area,
            bottom_area,
        };
    }

    let status_height = if has_status_banner { 1 } else { 0 };
    let gap_height = BODY_BOTTOM_GAP_HEIGHT;
    let reserved_bottom = bottom_height
        .saturating_add(status_height)
        .saturating_add(gap_height)
        .min(content.height);
    let available_body = content.height.saturating_sub(reserved_bottom);
    let visible_body = body_height.min(available_body);
    let desired_height = visible_body
        .saturating_add(status_height)
        .saturating_add(gap_height)
        .saturating_add(bottom_height)
        .min(content.height.max(1))
        .max(
            bottom_height
                .saturating_add(status_height)
                .saturating_add(gap_height)
                .max(1),
        );
    let stack_y = content.bottom().saturating_sub(desired_height);
    let stack_area = Rect::new(content.x, stack_y, content.width, desired_height);
    let [body_area, status_area, _gap_area, bottom_area] = Layout::vertical([
        Constraint::Length(visible_body.min(desired_height)),
        Constraint::Length(status_height.min(desired_height.saturating_sub(visible_body))),
        Constraint::Length(
            gap_height.min(
                desired_height
                    .saturating_sub(visible_body)
                    .saturating_sub(status_height),
            ),
        ),
        Constraint::Length(
            bottom_height.min(
                desired_height
                    .saturating_sub(visible_body)
                    .saturating_sub(status_height)
                    .saturating_sub(gap_height),
            ),
        ),
    ])
    .areas(stack_area);

    ChatSurfaceLayout {
        body_area,
        status_area,
        bottom_area,
    }
}

fn desired_stack_height(
    app: &TuiApp,
    area: Rect,
    body_height: u16,
    has_status_banner: bool,
    is_welcome: bool,
) -> u16 {
    let content = centered_column(area, MAX_CONTENT_WIDTH);
    let bottom_height = bottom_pane_height(app, content.width)
        .min(content.height)
        .max(1);
    if is_welcome {
        return content.height.max(1);
    }
    let status_height = if has_status_banner { 1 } else { 0 };
    let gap_height = BODY_BOTTOM_GAP_HEIGHT;
    let visible_body = body_height.min(available_body_height(
        content,
        bottom_height,
        status_height,
        gap_height,
    ));
    visible_body
        .saturating_add(status_height)
        .saturating_add(gap_height)
        .saturating_add(bottom_height)
        .min(content.height.max(1))
        .max(1)
}

fn bottom_pane_height(app: &TuiApp, width: u16) -> u16 {
    app.bottom_pane
        .desired_height(app.current_mode(), width)
        .max(1)
}

fn available_body_height(
    content: Rect,
    bottom_height: u16,
    status_height: u16,
    gap_height: u16,
) -> u16 {
    let reserved_bottom = bottom_height
        .saturating_add(status_height)
        .saturating_add(gap_height)
        .min(content.height);
    content.height.saturating_sub(reserved_bottom)
}

fn render_body_area(app: &TuiApp, frame: &mut Frame, area: Rect, model: ChatSurfaceModel) {
    match model.body {
        ChatSurfaceBody::Welcome => render_welcome(app, frame, area),
        ChatSurfaceBody::ActiveCell(active_cell) => {
            render_active_cell(frame, area, active_cell.lines)
        }
    }
}

fn render_status_area(
    frame: &mut Frame,
    area: Rect,
    live_banner: Option<&str>,
    live_banner_level: Option<NoticeLevel>,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let Some(live_banner) = live_banner else {
        return;
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            live_banner.to_string(),
            Style::default().fg(match live_banner_level {
                Some(NoticeLevel::Info) => Color::Rgb(120, 170, 235),
                Some(NoticeLevel::Warn) => Color::Rgb(230, 185, 80),
                Some(NoticeLevel::Error) => Color::Rgb(235, 120, 120),
                None => Color::Rgb(140, 140, 155),
            }),
        ))),
        area,
    );
}

fn render_active_cell(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    if area.height == 0 || area.width == 0 || lines.is_empty() {
        return;
    }
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 0,
    });
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let visible_lines = tail_visible_lines(lines, inner.height as usize);
    frame.render_widget(
        Paragraph::new(Text::from(visible_lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn tail_visible_lines(lines: Vec<Line<'static>>, max_lines: usize) -> Vec<Line<'static>> {
    if lines.len() <= max_lines {
        lines
    } else {
        lines[lines.len().saturating_sub(max_lines)..].to_vec()
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
            app.bottom_pane.build_status_view_model(app).text,
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
