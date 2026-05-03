use crate::app::TuiApp;
use crate::state::live_state::LivePhase;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

pub(crate) fn render_live_status_line(app: &TuiApp) -> Option<Line<'static>> {
    let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let dot = frames[(app.run_state.live_animation_frame as usize) % frames.len()];
    match &app.run_state.live_state.phase {
        LivePhase::Idle => None,
        LivePhase::AssistantResponding => Some(Line::from(vec![
            Span::styled(" ", Style::default().fg(Color::Rgb(90, 100, 120))),
            Span::styled(dot.to_string(), Style::default().fg(Color::Rgb(100, 180, 255))),
            Span::styled(
                " assistant is responding",
                Style::default().fg(Color::Rgb(140, 150, 170)),
            ),
        ])),
        LivePhase::Reasoning => Some(Line::from(vec![
            Span::styled(" ", Style::default().fg(Color::Rgb(90, 100, 120))),
            Span::styled(dot.to_string(), Style::default().fg(Color::Rgb(170, 140, 255))),
            Span::styled(" reasoning", Style::default().fg(Color::Rgb(140, 150, 170))),
        ])),
        LivePhase::ToolRunning { title } => Some(Line::from(vec![
            Span::styled(" ", Style::default().fg(Color::Rgb(90, 100, 120))),
            Span::styled(dot.to_string(), Style::default().fg(Color::Rgb(120, 190, 130))),
            Span::styled(
                format!(" running {title}"),
                Style::default().fg(Color::Rgb(140, 150, 170)),
            ),
        ])),
    }
}
