use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::input::intent::ComposerIntent;
use crate::input::slash_command::{SlashCommand, find_slash_command};
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crate::ui::widgets::textarea::TextArea;
use agent_protocol::{RequestId, ServerRequestDecisionKind};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct ServerRequestInlineState {
    pub request_id: RequestId,
    pub title: String,
    pub detail: String,
}

pub struct ServerRequestOverlay {
    state: ServerRequestInlineState,
    queue: VecDeque<ServerRequestInlineState>,
    reply: TextArea,
    complete: bool,
    selected: usize,
}

impl ServerRequestOverlay {
    pub fn new(state: ServerRequestInlineState) -> Self {
        Self {
            state,
            queue: VecDeque::new(),
            reply: TextArea::new(),
            complete: false,
            selected: 0,
        }
    }

    fn submit_current(
        &mut self,
        decision: ServerRequestDecisionKind,
        reason: String,
    ) -> BottomPaneViewAction {
        let request_id = self.state.request_id.clone();
        self.reply.clear();
        self.selected = 0;
        if let Some(next) = self.queue.pop_front() {
            self.state = next;
            self.complete = false;
        } else {
            self.complete = true;
        }
        BottomPaneViewAction::ServerRequestSubmit {
            request_id,
            decision,
            reason,
        }
    }
}

const REPLY_PROMPT_WIDTH: usize = 10;
const REPLY_LINE_INDEX: u16 = 5;
const COMPACT_APPROVAL_HEIGHT: u16 = 6;

impl BottomPaneView for ServerRequestOverlay {
    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if !matches!(key.kind, KeyEventKind::Press) {
            return BottomPaneViewAction::None;
        }

        match key.code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                BottomPaneViewAction::None
            }
            KeyCode::Down => {
                self.selected = self.selected.saturating_add(1).min(2);
                BottomPaneViewAction::None
            }
            KeyCode::Char('1') if self.reply.is_empty() => {
                self.submit_current(ServerRequestDecisionKind::Accept, String::new())
            }
            KeyCode::Char('2') if self.reply.is_empty() => {
                self.submit_current(ServerRequestDecisionKind::AcceptForSession, String::new())
            }
            KeyCode::Char('3') if self.reply.is_empty() => {
                self.submit_current(ServerRequestDecisionKind::Decline, String::new())
            }
            KeyCode::Char('y') if self.reply.is_empty() => {
                self.submit_current(ServerRequestDecisionKind::Accept, String::new())
            }
            KeyCode::Char('a') if self.reply.is_empty() => {
                self.submit_current(ServerRequestDecisionKind::AcceptForSession, String::new())
            }
            KeyCode::Char('n') if self.reply.is_empty() => {
                self.submit_current(ServerRequestDecisionKind::Decline, String::new())
            }
            KeyCode::Enter => {
                let reason = self.reply.take_trimmed();
                if let Some(intent) = slash_intent(&reason) {
                    return BottomPaneViewAction::Composer(intent);
                }
                let decision = if reason.is_empty() {
                    selected_decision(self.selected)
                } else {
                    typed_decision(&reason)
                };
                self.submit_current(decision, reason)
            }
            _ => {
                self.reply.handle_key(key);
                BottomPaneViewAction::None
            }
        }
    }

    fn render_lines(&self, area_width: u16) -> Vec<Line<'static>> {
        let accent = Color::Rgb(255, 184, 76);
        let soft = Color::Rgb(150, 150, 160);
        let title_bg = Color::Rgb(42, 34, 18);
        let option_bg = Color::Rgb(38, 42, 55);
        let title_width = area_width.saturating_sub(22) as usize;
        let detail_width = area_width.saturating_sub(4) as usize;
        let queue_label = if self.queue.is_empty() {
            String::new()
        } else {
            format!("  +{} queued", self.queue.len())
        };
        let option_style = |selected: bool| {
            if selected {
                Style::default()
                    .fg(Color::White)
                    .bg(option_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(180, 184, 200))
            }
        };
        let marker_style = |selected: bool, color: Color| {
            if selected {
                Style::default()
                    .fg(color)
                    .bg(option_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            }
        };

        vec![
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    " ACTION REQUIRED ",
                    Style::default()
                        .fg(accent)
                        .bg(title_bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    truncate_to_width(&format!("{}{}", self.state.title, queue_label), title_width),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    truncate_to_width(&self.state.detail, detail_width),
                    Style::default().fg(soft),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if self.selected == 0 { "› " } else { "  " },
                    marker_style(self.selected == 0, Color::Rgb(100, 255, 100)),
                ),
                Span::styled("Approve once", option_style(self.selected == 0)),
                Span::styled(
                    "  run this command one time",
                    Style::default().fg(Color::Rgb(92, 96, 118)),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if self.selected == 1 { "› " } else { "  " },
                    marker_style(self.selected == 1, Color::Rgb(100, 210, 255)),
                ),
                Span::styled("Approve for session", option_style(self.selected == 1)),
                Span::styled(
                    "  remember this tool permission",
                    Style::default().fg(Color::Rgb(92, 96, 118)),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if self.selected == 2 { "› " } else { "  " },
                    marker_style(self.selected == 2, Color::Rgb(255, 100, 100)),
                ),
                Span::styled("Deny", option_style(self.selected == 2)),
                Span::styled(
                    "  skip this tool call",
                    Style::default().fg(Color::Rgb(92, 96, 118)),
                ),
            ]),
            if self.reply.is_empty() {
                Line::from(Span::styled(
                    "  ↑/↓ select  ·  Enter confirm  ·  y approve  ·  a session  ·  n deny  ·  / commands",
                    Style::default().fg(Color::Rgb(62, 62, 78)),
                ))
            } else {
                Line::from(vec![
                    Span::styled(
                        "  note    ",
                        Style::default()
                            .fg(accent)
                            .bg(title_bg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        self.reply.text().to_string(),
                        Style::default().fg(Color::Rgb(220, 220, 230)),
                    ),
                ])
            },
        ]
    }

    fn desired_height(&self, _area_width: u16) -> u16 {
        COMPACT_APPROVAL_HEIGHT
    }

    fn cursor_position(&self, area: Rect) -> Option<(u16, u16)> {
        if self.reply.is_empty() {
            return None;
        }
        let content_width = area
            .width
            .saturating_sub(REPLY_PROMPT_WIDTH as u16 + 2)
            .max(1) as usize;
        let (cursor_row, cursor_col) = self.reply.visual_cursor_position(content_width);
        let mut x = area.x + (REPLY_PROMPT_WIDTH + cursor_col) as u16;
        let mut y = area.y + REPLY_LINE_INDEX + cursor_row as u16;
        if area.height > 0 {
            let max_y = area.y + area.height.saturating_sub(1);
            if y > max_y {
                y = max_y;
            }
        }
        if area.width > 0 {
            let max_x = area.x + area.width.saturating_sub(1);
            if x > max_x {
                x = max_x;
            }
        }
        Some((x, y))
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn try_consume_server_request(
        &mut self,
        request: ServerRequestInlineState,
    ) -> Option<ServerRequestInlineState> {
        self.queue.push_back(request);
        None
    }

    fn dismiss_server_request(&mut self, request_id: &RequestId) -> bool {
        let before = self.queue.len();
        self.queue
            .retain(|request| &request.request_id != request_id);
        if &self.state.request_id == request_id {
            if let Some(next) = self.queue.pop_front() {
                self.state = next;
                self.reply.clear();
                self.selected = 0;
                self.complete = false;
            } else {
                self.complete = true;
            }
            return true;
        }
        before != self.queue.len()
    }

    fn active_server_request_id(&self) -> Option<&RequestId> {
        (!self.complete).then_some(&self.state.request_id)
    }

    fn requires_action(&self) -> bool {
        !self.complete
    }
}

fn truncate_to_width(value: &str, max_width: usize) -> String {
    if value.width() <= max_width {
        return value.to_string();
    }
    let ellipsis_width = 1usize;
    let allowed = max_width.saturating_sub(ellipsis_width);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let ch_width = ch.to_string().width();
        if used + ch_width > allowed {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    out
}

fn selected_decision(selected: usize) -> ServerRequestDecisionKind {
    match selected {
        0 => ServerRequestDecisionKind::Accept,
        1 => ServerRequestDecisionKind::AcceptForSession,
        _ => ServerRequestDecisionKind::Decline,
    }
}

fn typed_decision(reason: &str) -> ServerRequestDecisionKind {
    if reason.eq_ignore_ascii_case("n") || reason.eq_ignore_ascii_case("no") {
        ServerRequestDecisionKind::Decline
    } else if reason.eq_ignore_ascii_case("a")
        || reason.eq_ignore_ascii_case("all")
        || reason.eq_ignore_ascii_case("session")
    {
        ServerRequestDecisionKind::AcceptForSession
    } else {
        ServerRequestDecisionKind::Accept
    }
}

fn slash_intent(line: &str) -> Option<ComposerIntent> {
    let command_text = line.strip_prefix('/')?;
    let mut parts = command_text.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or_default();
    let args = parts.next().unwrap_or_default().trim();
    let Some(command) = find_slash_command(name) else {
        return Some(ComposerIntent::UnknownCommand(name.to_string()));
    };
    if !args.is_empty() && !command.supports_inline_args() {
        return Some(ComposerIntent::UnknownCommand(name.to_string()));
    }
    Some(intent_for_command(command))
}

fn intent_for_command(command: SlashCommand) -> ComposerIntent {
    match command {
        SlashCommand::Clear => ComposerIntent::Reset,
        SlashCommand::Copy => ComposerIntent::Copy,
        SlashCommand::Help => ComposerIntent::Help,
        SlashCommand::Interrupt => ComposerIntent::Interrupt,
        SlashCommand::Exit => ComposerIntent::Exit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn request_state(id: &str) -> ServerRequestInlineState {
        ServerRequestInlineState {
            request_id: RequestId::String(id.to_string()),
            title: "Run command?".to_string(),
            detail: "shell_command".to_string(),
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_text(overlay: &mut ServerRequestOverlay, text: &str) {
        for ch in text.chars() {
            overlay.handle_key_event(key(KeyCode::Char(ch)));
        }
    }

    #[test]
    fn cursor_is_hidden_until_a_note_is_typed() {
        let mut overlay = ServerRequestOverlay::new(request_state("req-1"));

        assert!(overlay.cursor_position(Rect::new(0, 20, 80, 16)).is_none());

        type_text(&mut overlay, "because");

        let (_x, y) = overlay
            .cursor_position(Rect::new(0, 20, 80, 16))
            .expect("cursor");

        assert_eq!(y, 25);
    }

    #[test]
    fn numeric_shortcuts_submit_matching_decisions() {
        let mut overlay = ServerRequestOverlay::new(request_state("req-1"));

        let action = overlay.handle_key_event(key(KeyCode::Char('3')));

        assert!(matches!(
            action,
            BottomPaneViewAction::ServerRequestSubmit {
                decision: ServerRequestDecisionKind::Decline,
                ..
            }
        ));
    }

    #[test]
    fn slash_command_in_request_overlay_dispatches_global_intent() {
        let mut overlay = ServerRequestOverlay::new(request_state("req-1"));
        type_text(&mut overlay, "/interrupt");

        let action = overlay.handle_key_event(key(KeyCode::Enter));

        assert!(matches!(
            action,
            BottomPaneViewAction::Composer(ComposerIntent::Interrupt)
        ));
    }

    #[test]
    fn slash_unknown_in_request_overlay_is_not_treated_as_approval_reason() {
        let mut overlay = ServerRequestOverlay::new(request_state("req-1"));
        type_text(&mut overlay, "/wat");

        let action = overlay.handle_key_event(key(KeyCode::Enter));

        assert!(matches!(
            action,
            BottomPaneViewAction::Composer(ComposerIntent::UnknownCommand(command))
                if command == "wat"
        ));
    }
}
