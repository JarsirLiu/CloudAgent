use crate::app::TuiApp;
use crate::state::runtime_projection::RuntimePhase;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

pub(crate) fn render_live_status_line(app: &TuiApp) -> Option<Line<'static>> {
    let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let dot = frames[(app.run_state.live_animation_frame as usize) % frames.len()];
    if let Some(label) = app.runtime_projection.live_label.as_ref() {
        let color = match app.runtime_projection.phase.as_ref() {
            Some(RuntimePhase::ToolRunning) => Color::Rgb(120, 190, 130),
            Some(RuntimePhase::WaitingApproval) => Color::Rgb(255, 190, 90),
            _ => Color::Rgb(100, 180, 255),
        };
        return Some(Line::from(vec![
            Span::styled(" ", Style::default().fg(Color::Rgb(90, 100, 120))),
            Span::styled(dot.to_string(), Style::default().fg(color)),
            Span::styled(format!(" {label}"), Style::default().fg(Color::Rgb(140, 150, 170))),
        ]));
    }

    match app.runtime_projection.phase.as_ref() {
        Some(RuntimePhase::ModelStreaming) => Some(Line::from(vec![
            Span::styled(" ", Style::default().fg(Color::Rgb(90, 100, 120))),
            Span::styled(dot.to_string(), Style::default().fg(Color::Rgb(100, 180, 255))),
            Span::styled(
                " assistant is responding",
                Style::default().fg(Color::Rgb(140, 150, 170)),
            ),
        ])),
        Some(RuntimePhase::WaitingApproval) => Some(Line::from(vec![
            Span::styled(" ", Style::default().fg(Color::Rgb(90, 100, 120))),
            Span::styled(dot.to_string(), Style::default().fg(Color::Rgb(255, 190, 90))),
            Span::styled(
                " waiting for approval",
                Style::default().fg(Color::Rgb(140, 150, 170)),
            ),
        ])),
        Some(RuntimePhase::ToolRunning) => Some(Line::from(vec![
            Span::styled(" ", Style::default().fg(Color::Rgb(90, 100, 120))),
            Span::styled(dot.to_string(), Style::default().fg(Color::Rgb(120, 190, 130))),
            Span::styled(" running tool", Style::default().fg(Color::Rgb(140, 150, 170))),
        ])),
        _ => None,
    }
}
