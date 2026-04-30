use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crate::ui::widgets::chat_composer::ChatComposer;
use crate::ui::widgets::footer::{hint_line, status_line};
pub use crate::ui::widgets::server_request_overlay::ServerRequestInlineState;
use crate::ui::widgets::server_request_overlay::ServerRequestOverlay;
use agent_protocol::{FrontendMode, RequestId, ServerRequestDecisionKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

pub struct InputPane {
    composer: ChatComposer,
    view_stack: Vec<Box<dyn BottomPaneView>>,
}

pub(crate) enum InputPaneAction {
    Composer(ComposerIntent),
    ServerRequestSubmit {
        request_id: RequestId,
        decision: ServerRequestDecisionKind,
        reason: String,
    },
}

impl InputPane {
    pub fn new() -> Self {
        Self {
            composer: ChatComposer::new(),
            view_stack: Vec::new(),
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<InputPaneAction> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('k') {
            return Some(InputPaneAction::Composer(ComposerIntent::Interrupt));
        }

        if let Some(view) = self.view_stack.last_mut() {
            match view.handle_key_event(key) {
                BottomPaneViewAction::None => {}
                BottomPaneViewAction::Close => {
                    self.view_stack.pop();
                    return Some(InputPaneAction::Composer(ComposerIntent::None));
                }
                BottomPaneViewAction::Composer(intent) => {
                    if !matches!(intent, ComposerIntent::None) {
                        return Some(InputPaneAction::Composer(intent));
                    }
                }
                BottomPaneViewAction::ServerRequestSubmit {
                    request_id,
                    decision,
                    reason,
                } => {
                    if view.is_complete() {
                        self.view_stack.pop();
                    }
                    return Some(InputPaneAction::ServerRequestSubmit {
                        request_id,
                        decision,
                        reason,
                    });
                }
            }
            if view.is_complete() {
                self.view_stack.pop();
            }
            return None;
        }
        self.composer.handle_key(key).map(InputPaneAction::Composer)
    }

    pub(crate) fn handle_paste(&mut self, text: &str) -> Option<InputPaneAction> {
        if self.view_stack.is_empty() {
            return Some(InputPaneAction::Composer(self.composer.handle_paste(text)));
        }
        None
    }

    pub fn render(
        &self,
        mode: FrontendMode,
        status_text: &str,
        status_meta: &str,
        area_width: u16,
    ) -> (Paragraph<'static>, u16, u16) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let inner_width = area_width.saturating_sub(2) as usize;
        lines.push(status_line(mode, status_text, status_meta, inner_width));

        let mut lines_before_composer = 1u16;

        if let Some(view) = self.view_stack.last() {
            lines.push(Line::raw(""));
            lines_before_composer += 1;
            let view_lines = view.render_lines(area_width.saturating_sub(2));
            lines_before_composer += view_lines.len() as u16;
            lines.extend(view_lines);
        }

        if self.view_stack.is_empty() {
            let composer = self.composer.render(mode, inner_width);
            lines.push(Line::raw(""));
            lines_before_composer += 1;
            lines_before_composer += composer.cursor_row;
            lines.extend(composer.lines);
            if composer.completion_lines.is_empty() {
                lines.push(Line::raw(""));
                lines.push(hint_line(mode, inner_width));
            } else {
                lines.extend(composer.completion_lines);
            }
        }

        let total_lines = lines.len() as u16;
        let border_style = match mode {
            FrontendMode::Idle => Style::default().fg(Color::Rgb(75, 82, 110)),
            FrontendMode::Running => Style::default().fg(Color::Rgb(82, 130, 190)),
            FrontendMode::WaitingForServerRequest => Style::default().fg(Color::Rgb(210, 150, 45)),
        };
        (
            Paragraph::new(Text::from(lines)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border_style)
                    .title_style(
                        Style::default()
                            .fg(Color::Rgb(215, 220, 235))
                            .add_modifier(Modifier::BOLD),
                    )
                    .title(" composer "),
            ),
            lines_before_composer,
            total_lines,
        )
    }

    pub fn desired_height(&self, mode: FrontendMode, area_width: u16) -> u16 {
        let inner_width = area_width.saturating_sub(2) as usize;
        let mut total = 3u16; // border + status row
        if let Some(view) = self.view_stack.last() {
            total += 1;
            total += view.desired_height(area_width.saturating_sub(2));
        } else {
            let composer = self.composer.render(mode, inner_width);
            total += 1; // gap before composer
            total += composer.lines.len() as u16;
            total += 2; // gap + hint line
        }
        let min_height = if !self.view_stack.is_empty() {
            7
        } else if self.composer.has_completion_menu() {
            9
        } else {
            7
        };
        total.max(min_height)
    }

    pub fn cursor_position(
        &self,
        area: Rect,
        lines_before: u16,
        mode: FrontendMode,
    ) -> Option<(u16, u16)> {
        let inner = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1).saturating_add(lines_before),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(lines_before + 2),
        };
        if let Some(view) = self.view_stack.last() {
            let view_area = Rect {
                x: area.x.saturating_add(1),
                y: area.y.saturating_add(3),
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(4),
            };
            return view.cursor_position(view_area);
        }
        Some(self.composer.cursor_position(inner, mode))
    }

    pub fn clear_views(&mut self) {
        self.view_stack.clear();
    }

    pub fn set_server_request(&mut self, request: ServerRequestInlineState) {
        let request = if let Some(view) = self.view_stack.last_mut() {
            match view.try_consume_server_request(request) {
                Some(request) => request,
                None => return,
            }
        } else {
            request
        };

        self.view_stack.clear();
        self.view_stack
            .push(Box::new(ServerRequestOverlay::new(request)));
    }

    pub fn clear_server_request(&mut self) {
        self.view_stack.clear();
    }

    pub fn dismiss_server_request(&mut self, request_id: &RequestId) {
        let Some(view) = self.view_stack.last_mut() else {
            return;
        };
        if !view.dismiss_server_request(request_id) {
            return;
        }
        if view.is_complete() {
            self.view_stack.pop();
        }
    }

    pub fn active_server_request_id(&self) -> Option<RequestId> {
        self.view_stack
            .last()
            .and_then(|view| view.active_server_request_id().cloned())
    }

    pub fn requires_action(&self) -> bool {
        self.view_stack
            .last()
            .is_some_and(|view| view.requires_action())
    }

    pub fn composer_is_empty(&self) -> bool {
        self.view_stack.is_empty() && self.composer.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventKind, KeyModifiers};

    #[test]
    fn ctrl_k_interrupts_even_when_server_request_overlay_is_active() {
        let mut pane = InputPane::new();
        pane.set_server_request(ServerRequestInlineState {
            request_id: RequestId::String("req-1".to_string()),
            title: "Run command?".to_string(),
            detail: "shell_command".to_string(),
        });

        let action = pane.handle_key(KeyEvent {
            code: KeyCode::Char('k'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });

        assert!(matches!(
            action,
            Some(InputPaneAction::Composer(ComposerIntent::Interrupt))
        ));
    }

    #[test]
    fn server_request_overlay_queues_new_requests_instead_of_replacing_current() {
        let mut pane = InputPane::new();
        pane.set_server_request(ServerRequestInlineState {
            request_id: RequestId::String("req-1".to_string()),
            title: "First command".to_string(),
            detail: "shell_command".to_string(),
        });
        pane.set_server_request(ServerRequestInlineState {
            request_id: RequestId::String("req-2".to_string()),
            title: "Second command".to_string(),
            detail: "shell_command".to_string(),
        });

        let first = pane.handle_key(KeyEvent {
            code: KeyCode::Char('1'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        let second = pane.handle_key(KeyEvent {
            code: KeyCode::Char('3'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });

        assert!(matches!(
            first,
            Some(InputPaneAction::ServerRequestSubmit {
                request_id: RequestId::String(id),
                decision: ServerRequestDecisionKind::Accept,
                ..
            }) if id == "req-1"
        ));
        assert!(matches!(
            second,
            Some(InputPaneAction::ServerRequestSubmit {
                request_id: RequestId::String(id),
                decision: ServerRequestDecisionKind::Decline,
                ..
            }) if id == "req-2"
        ));
    }

    #[test]
    fn queued_server_request_remains_action_required_after_first_submit() {
        let mut pane = InputPane::new();
        pane.set_server_request(ServerRequestInlineState {
            request_id: RequestId::String("req-1".to_string()),
            title: "First command".to_string(),
            detail: "shell_command".to_string(),
        });
        pane.set_server_request(ServerRequestInlineState {
            request_id: RequestId::String("req-2".to_string()),
            title: "Second command".to_string(),
            detail: "shell_command".to_string(),
        });

        let _ = pane.handle_key(KeyEvent {
            code: KeyCode::Char('1'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });

        assert!(pane.requires_action());
        assert_eq!(
            pane.active_server_request_id(),
            Some(RequestId::String("req-2".to_string()))
        );
    }

    #[test]
    fn approval_selection_mode_does_not_force_a_text_cursor() {
        let mut pane = InputPane::new();
        pane.set_server_request(ServerRequestInlineState {
            request_id: RequestId::String("req-1".to_string()),
            title: "Run command?".to_string(),
            detail: "shell_command".to_string(),
        });

        assert_eq!(
            pane.cursor_position(
                Rect::new(0, 20, 100, 8),
                1,
                FrontendMode::WaitingForServerRequest
            ),
            None
        );
    }

    #[test]
    fn idle_composer_stays_compact_and_completion_gets_menu_space() {
        let mut pane = InputPane::new();
        let before = pane.desired_height(FrontendMode::Idle, 100);
        assert_eq!(before, 7);

        let _ = pane.handle_key(KeyEvent {
            code: KeyCode::Char('/'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });

        let after = pane.desired_height(FrontendMode::Idle, 100);
        assert!(after > before);
        let (_, _, total_lines) = pane.render(FrontendMode::Idle, "Idle", "test", 100);
        assert!(total_lines > 5);
    }
}
