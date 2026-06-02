use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crate::ui::widgets::form_input_state::FormInputState;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct ConfigPanel {
    selected: usize,
    api_key: String,
    base_url: String,
    model: String,
    input_state: FormInputState,
}

impl ConfigPanel {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            selected: 0,
            api_key,
            base_url,
            model,
            input_state: FormInputState::new(),
        }
    }

    fn move_selection(&mut self, next: usize) {
        self.input_state.move_selection(&mut self.selected, next.min(3));
    }
}

impl BottomPaneView for ConfigPanel {
    fn should_capture_global_paste_shortcut(&self) -> bool {
        false
    }

    fn handle_paste(&mut self, text: &str) -> BottomPaneViewAction {
        let value = text.replace('\n', "");
        match self.selected {
            0 => self.input_state.append_paste(&mut self.api_key, &value),
            1 => self.input_state.append_paste(&mut self.base_url, &value),
            2 => self.input_state.append_paste(&mut self.model, &value),
            _ => {}
        }
        BottomPaneViewAction::None
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if !matches!(key.kind, KeyEventKind::Press) {
            return BottomPaneViewAction::None;
        }
        match key.code {
            KeyCode::Up => self.move_selection(self.selected.saturating_sub(1)),
            KeyCode::Down | KeyCode::Tab => self.move_selection(self.selected + 1),
            KeyCode::BackTab => self.move_selection(self.selected.saturating_sub(1)),
            KeyCode::Backspace => {
                match self.selected {
                    0 => self.input_state.backspace(&mut self.api_key),
                    1 => self.input_state.backspace(&mut self.base_url),
                    2 => self.input_state.backspace(&mut self.model),
                    _ => {}
                }
            }
            KeyCode::Char(c) => {
                match self.selected {
                    0 => {
                        let _ = self.input_state.append_char(&mut self.api_key, c);
                    }
                    1 => {
                        let _ = self.input_state.append_char(&mut self.base_url, c);
                    }
                    2 => {
                        let _ = self.input_state.append_char(&mut self.model, c);
                    }
                    _ => {}
                }
            }
            KeyCode::Enter => {
                if self.selected == 3 {
                    return BottomPaneViewAction::Composer(ComposerIntent::ConfigSave {
                        api_key: self.api_key.trim().to_string(),
                        base_url: self.base_url.trim().to_string(),
                        model: self.model.trim().to_string(),
                    });
                }
                self.move_selection(self.selected + 1);
            }
            KeyCode::Esc => return BottomPaneViewAction::Close,
            _ => {}
        }
        BottomPaneViewAction::None
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let row_style = |selected: bool| {
            if selected {
                Style::default()
                    .fg(Color::Rgb(190, 220, 255))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(140, 150, 180))
            }
        };
        vec![
            Line::from("  Config Panel"),
            Line::from("  Type values, Tab/Up/Down switch fields, paste directly, Enter on Save"),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if self.selected == 0 {
                        "> API Key  : "
                    } else {
                        "  API Key  : "
                    },
                    row_style(self.selected == 0),
                ),
                Span::styled(
                    if self.api_key.is_empty() {
                        "(empty)".to_string()
                    } else {
                        self.api_key.clone()
                    },
                    Style::default().fg(Color::Rgb(210, 215, 225)),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if self.selected == 1 {
                        "> Base URL : "
                    } else {
                        "  Base URL : "
                    },
                    row_style(self.selected == 1),
                ),
                Span::styled(
                    if self.base_url.is_empty() {
                        "(empty)".to_string()
                    } else {
                        self.base_url.clone()
                    },
                    Style::default().fg(Color::Rgb(210, 215, 225)),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if self.selected == 2 {
                        "> Model    : "
                    } else {
                        "  Model    : "
                    },
                    row_style(self.selected == 2),
                ),
                Span::styled(
                    if self.model.is_empty() {
                        "(empty)".to_string()
                    } else {
                        self.model.clone()
                    },
                    Style::default().fg(Color::Rgb(210, 215, 225)),
                ),
            ]),
            Line::from(Span::styled(
                if self.selected == 3 {
                    "  > [ Save ]"
                } else {
                    "    [ Save ]"
                },
                row_style(self.selected == 3),
            )),
        ]
    }

    fn cursor_position(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, KeyEventState};

    fn key(ch: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn ignores_immediate_char_echo_after_paste() {
        let mut panel = ConfigPanel::new(String::new(), String::new(), String::new());
        let _ = panel.handle_paste("token123");
        for ch in "token123".chars() {
            let _ = panel.handle_key_event(key(ch));
        }
        assert_eq!(panel.api_key, "token123");
    }

    #[test]
    fn keeps_regular_typed_input() {
        let mut panel = ConfigPanel::new(String::new(), String::new(), String::new());
        let _ = panel.handle_key_event(key('a'));
        let _ = panel.handle_key_event(key('b'));
        let _ = panel.handle_key_event(key('c'));
        assert_eq!(panel.api_key, "abc");
    }
}
