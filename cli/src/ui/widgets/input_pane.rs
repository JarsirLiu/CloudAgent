use crate::input::intent::ComposerIntent;
use crate::terminal::Frame;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crate::ui::widgets::chat_composer::ChatComposer;
use crate::ui::widgets::config_panel::ConfigPanel;
use crate::ui::widgets::filter_picker::FilterPicker;
use crate::ui::widgets::footer::{hint_line, status_line};
use crate::ui::widgets::permissions_picker::PermissionsPicker;
pub use crate::ui::widgets::server_request_overlay::ServerRequestInlineState;
use crate::ui::widgets::server_request_overlay::ServerRequestOverlay;
use crate::ui::widgets::session_picker::{SessionPicker, SessionPickerMode};
use agent_protocol::{ConversationSummary, FrontendMode, RequestId, ServerRequestDecisionKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

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

pub(crate) struct InputPaneRenderResult {
    pub cursor_position: Option<(u16, u16)>,
}

impl InputPane {
    pub fn new() -> Self {
        Self {
            composer: ChatComposer::new(),
            view_stack: Vec::new(),
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<InputPaneAction> {
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
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
                        self.view_stack.pop();
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
        if let Some(view) = self.view_stack.last_mut() {
            let action = view.handle_paste(text);
            return match action {
                BottomPaneViewAction::Composer(intent) => Some(InputPaneAction::Composer(intent)),
                _ => None,
            };
        }
        Some(InputPaneAction::Composer(self.composer.handle_paste(text)))
    }

    pub(crate) fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        mode: FrontendMode,
        status_text: &str,
        status_meta: &str,
        hint_meta: &str,
    ) -> InputPaneRenderResult {
        if self
            .view_stack
            .last()
            .is_some_and(|view| view.requires_action())
        {
            let (widget, lines_before_composer, _) =
                self.render_request_view(mode, status_text, status_meta, area.width);
            frame.render_widget(widget, area);
            return InputPaneRenderResult {
                cursor_position: self.cursor_position(area, lines_before_composer, mode),
            };
        }

        let inner_width = area.width.saturating_sub(2) as usize;
        let composer = self.composer.render(mode, inner_width);
        let border_style = border_style(mode);
        let completion_lines = if let Some(view) = self.view_stack.last() {
            view.render_lines(area.width.saturating_sub(2))
        } else {
            composer.completion_lines.clone()
        };

        let status = status_line(mode, status_text, status_meta, inner_width);

        if completion_lines.is_empty() {
            let mut lines = Vec::new();
            lines.push(status);
            lines.push(Line::raw(""));
            lines.extend(composer.lines);
            lines.push(hint_line(mode, inner_width, hint_meta));

            let widget = input_block(lines, border_style);
            frame.render_widget(widget, area);
            let composer_area = composer_area(area, 2);
            return InputPaneRenderResult {
                cursor_position: Some(self.composer.cursor_position(composer_area, mode)),
            };
        }

        let input_height = 5u16.saturating_add(composer.cursor_row);
        let panel_height = completion_lines.len() as u16 + 1;
        let [input_area, completion_area] = Layout::vertical([
            Constraint::Length(input_height),
            Constraint::Min(panel_height),
        ])
        .areas(area);

        let mut input_lines = vec![status];
        input_lines.push(Line::raw(""));
        input_lines.extend(composer.lines);
        frame.render_widget(input_block(input_lines, border_style), input_area);

        let panel = Paragraph::new(Text::from(completion_lines))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::Rgb(58, 64, 86))),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(panel, completion_area);

        InputPaneRenderResult {
            cursor_position: Some(
                self.composer
                    .cursor_position(composer_area(input_area, 2), mode),
            ),
        }
    }

    fn render_request_view(
        &self,
        mode: FrontendMode,
        status_text: &str,
        _status_meta: &str,
        area_width: u16,
    ) -> (Paragraph<'static>, u16, u16) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let inner_width = area_width.saturating_sub(2) as usize;
        lines.push(status_line(mode, status_text, "", inner_width));

        let mut lines_before_composer = 1u16;

        if let Some(view) = self.view_stack.last() {
            lines.push(Line::raw(""));
            lines_before_composer += 1;
            let view_lines = view.render_lines(area_width.saturating_sub(2));
            lines_before_composer += view_lines.len() as u16;
            lines.extend(view_lines);
        }

        let total_lines = lines.len() as u16;
        (
            Paragraph::new(Text::from(lines)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border_style(mode))
                    .title_style(
                        Style::default()
                            .fg(Color::Rgb(215, 220, 235))
                            .add_modifier(Modifier::BOLD),
                    )
                    .title(" action "),
            ),
            lines_before_composer,
            total_lines,
        )
    }

    #[cfg(test)]
    fn render_lines_for_test(
        &self,
        mode: FrontendMode,
        status_text: &str,
        status_meta: &str,
        area_width: u16,
    ) -> (Vec<Line<'static>>, u16) {
        if self
            .view_stack
            .last()
            .is_some_and(|view| view.requires_action())
        {
            let (widget, lines_before, _) =
                self.render_request_view(mode, status_text, status_meta, area_width);
            let text = format!("{widget:?}");
            return (vec![Line::raw(text)], lines_before);
        }

        let inner_width = area_width.saturating_sub(2) as usize;
        let composer = self.composer.render(mode, inner_width);
        let mut lines = vec![status_line(mode, status_text, status_meta, inner_width)];
        lines.push(Line::raw(""));
        lines.extend(composer.lines);
        if !composer.completion_lines.is_empty() {
            lines.extend(composer.completion_lines);
        }
        (lines, 1 + composer.cursor_row)
    }

    pub fn desired_height(&self, mode: FrontendMode, area_width: u16) -> u16 {
        let inner_width = area_width.saturating_sub(2) as usize;
        if let Some(view) = self.view_stack.last()
            && view.requires_action()
        {
            return (4 + view.desired_height(area_width.saturating_sub(2))).max(7);
        }

        let composer = self.composer.render(mode, inner_width);
        let popup_height = if let Some(view) = self.view_stack.last() {
            view.desired_height(area_width.saturating_sub(2))
        } else {
            composer.completion_lines.len() as u16
        };
        if popup_height == 0 {
            // Border + status + spacer + input + hint.
            (5 + composer.lines.len() as u16).max(6)
        } else {
            // Small input surface with status plus an independent command panel below it.
            (5 + composer.cursor_row).saturating_add(popup_height + 1)
        }
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

    pub fn set_session_picker(
        &mut self,
        sessions: Vec<ConversationSummary>,
        active_conversation_id: &str,
        mode: SessionPickerMode,
    ) {
        self.view_stack.clear();
        self.view_stack.push(Box::new(SessionPicker::new(
            sessions,
            active_conversation_id,
            mode,
        )));
    }

    pub fn clear_session_picker(&mut self) {
        self.view_stack.retain(|view| {
            !view
                .render_lines(80)
                .first()
                .map(|line| line.to_string().contains("Session Picker"))
                .unwrap_or(false)
        });
    }

    pub fn set_filter_picker(&mut self) {
        self.view_stack.clear();
        self.view_stack.push(Box::new(FilterPicker::new()));
    }

    pub fn set_permissions_picker(&mut self, current: &str) {
        self.view_stack.clear();
        self.view_stack
            .push(Box::new(PermissionsPicker::new(current)));
    }

    pub fn set_config_panel(&mut self, api_key: String, base_url: String, model: String) {
        self.view_stack.clear();
        self.view_stack
            .push(Box::new(ConfigPanel::new(api_key, base_url, model)));
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

fn border_style(mode: FrontendMode) -> Style {
    match mode {
        FrontendMode::Idle => Style::default().fg(Color::Rgb(75, 82, 110)),
        FrontendMode::Running => Style::default().fg(Color::Rgb(82, 130, 190)),
        FrontendMode::WaitingForServerRequest => Style::default().fg(Color::Rgb(210, 150, 45)),
    }
}

fn input_block(lines: Vec<Line<'static>>, border_style: Style) -> Paragraph<'static> {
    Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border_style)
                .title_style(
                    Style::default()
                        .fg(Color::Rgb(215, 220, 235))
                        .add_modifier(Modifier::BOLD),
                )
                .title(" prompt "),
        )
        .wrap(Wrap { trim: false })
}

fn composer_area(area: Rect, content_row_offset: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1).saturating_add(content_row_offset),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(content_row_offset + 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventKind, KeyModifiers};

    #[test]
    fn ctrl_c_interrupts_even_when_server_request_overlay_is_active() {
        let mut pane = InputPane::new();
        pane.set_server_request(ServerRequestInlineState {
            request_id: RequestId::String("req-1".to_string()),
            title: "Run command?".to_string(),
            detail: "exec_command".to_string(),
        });

        let action = pane.handle_key(KeyEvent {
            code: KeyCode::Char('c'),
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
            detail: "exec_command".to_string(),
        });
        pane.set_server_request(ServerRequestInlineState {
            request_id: RequestId::String("req-2".to_string()),
            title: "Second command".to_string(),
            detail: "exec_command".to_string(),
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
            detail: "exec_command".to_string(),
        });
        pane.set_server_request(ServerRequestInlineState {
            request_id: RequestId::String("req-2".to_string()),
            title: "Second command".to_string(),
            detail: "exec_command".to_string(),
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
            detail: "exec_command".to_string(),
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
        assert_eq!(before, 6);

        let _ = pane.handle_key(KeyEvent {
            code: KeyCode::Char('/'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });

        let after = pane.desired_height(FrontendMode::Idle, 100);
        assert!(after > before);
        let (lines, _) = pane.render_lines_for_test(FrontendMode::Idle, "Idle", "test", 100);
        assert!(lines.len() > 5);
    }
}
