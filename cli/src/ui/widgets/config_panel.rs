use crate::input::intent::ComposerIntent;
use crate::text_width::display_width;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crate::ui::widgets::form_text_field::FormTextField;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct ConfigPanel {
    selected: usize,
    api_key: FormTextField,
    base_url: FormTextField,
    model: FormTextField,
}

impl ConfigPanel {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            selected: 0,
            api_key: FormTextField::new(api_key),
            base_url: FormTextField::new(base_url),
            model: FormTextField::new(model),
        }
    }

    fn move_selection(&mut self, next: usize) {
        self.selected = next.min(3);
    }

    fn active_field_mut(&mut self) -> Option<&mut FormTextField> {
        match self.selected {
            0 => Some(&mut self.api_key),
            1 => Some(&mut self.base_url),
            2 => Some(&mut self.model),
            _ => None,
        }
    }

    fn active_field(&self) -> Option<&FormTextField> {
        match self.selected {
            0 => Some(&self.api_key),
            1 => Some(&self.base_url),
            2 => Some(&self.model),
            _ => None,
        }
    }
}

impl BottomPaneView for ConfigPanel {
    fn should_capture_global_paste_shortcut(&self) -> bool {
        false
    }

    fn supports_text_paste_shortcut(&self) -> bool {
        true
    }

    fn handle_paste(&mut self, text: &str) -> BottomPaneViewAction {
        if let Some(field) = self.active_field_mut() {
            let _ = field.append_paste(text);
        }
        BottomPaneViewAction::None
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if !matches!(key.kind, KeyEventKind::Press) {
            return BottomPaneViewAction::None;
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('a') {
            if let Some(field) = self.active_field_mut() {
                field.select_all();
            }
            return BottomPaneViewAction::None;
        }
        match key.code {
            KeyCode::Up => self.move_selection(self.selected.saturating_sub(1)),
            KeyCode::Down | KeyCode::Tab => self.move_selection(self.selected + 1),
            KeyCode::BackTab => self.move_selection(self.selected.saturating_sub(1)),
            KeyCode::Left => {
                if let Some(field) = self.active_field_mut() {
                    field.move_left();
                }
            }
            KeyCode::Right => {
                if let Some(field) = self.active_field_mut() {
                    field.move_right();
                }
            }
            KeyCode::Home => {
                if let Some(field) = self.active_field_mut() {
                    field.move_to_start();
                }
            }
            KeyCode::End => {
                if let Some(field) = self.active_field_mut() {
                    field.move_to_end();
                }
            }
            KeyCode::Backspace => {
                if let Some(field) = self.active_field_mut() {
                    field.backspace();
                }
            }
            KeyCode::Delete => {
                if let Some(field) = self.active_field_mut() {
                    field.delete();
                }
            }
            KeyCode::Char(c) => {
                if let Some(field) = self.active_field_mut() {
                    let _ = field.append_char(c);
                }
            }
            KeyCode::Enter => {
                if self.selected == 3 {
                    return BottomPaneViewAction::Composer(ComposerIntent::ConfigSave {
                        api_key: self.api_key.value().trim().to_string(),
                        base_url: self.base_url.value().trim().to_string(),
                        model: self.model.value().trim().to_string(),
                    });
                }
                self.move_selection(self.selected + 1);
            }
            KeyCode::Esc => return BottomPaneViewAction::Cancel,
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
                    if self.api_key.is_empty() && self.selected != 0 {
                        "(empty)".to_string()
                    } else {
                        self.api_key.value().to_string()
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
                    if self.base_url.is_empty() && self.selected != 1 {
                        "(empty)".to_string()
                    } else {
                        self.base_url.value().to_string()
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
                    if self.model.is_empty() && self.selected != 2 {
                        "(empty)".to_string()
                    } else {
                        self.model.value().to_string()
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

    fn cursor_position(&self, area: Rect) -> Option<(u16, u16)> {
        let (row_offset, prefix) = match self.selected {
            0 => (2u16, "  > API Key  : "),
            1 => (3u16, "  > Base URL : "),
            2 => (4u16, "  > Model    : "),
            _ => return None,
        };
        Some((
            area.x
                .saturating_add(display_width(prefix) as u16)
                .saturating_add(
                    self.active_field()
                        .map(FormTextField::cursor_display_column)
                        .unwrap_or(0) as u16,
                ),
            area.y.saturating_add(row_offset),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventState, KeyModifiers};

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
        assert_eq!(panel.api_key.value(), "token123");
    }

    #[test]
    fn paste_appends_at_cursor_by_default() {
        let mut panel = ConfigPanel::new("old".to_string(), String::new(), String::new());
        let _ = panel.handle_key_event(KeyEvent {
            code: KeyCode::End,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = panel.handle_paste("token123");
        assert_eq!(panel.api_key.value(), "oldtoken123");
    }

    #[test]
    fn first_paste_after_activation_replaces_existing_field_value() {
        let mut panel = ConfigPanel::new("old".to_string(), String::new(), String::new());
        let _ = panel.handle_key_event(KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = panel.handle_paste("token123");
        assert_eq!(panel.api_key.value(), "token123");
    }

    #[test]
    fn ctrl_a_then_paste_replaces_existing_field_value() {
        let mut panel = ConfigPanel::new("old".to_string(), String::new(), String::new());
        let _ = panel.handle_key_event(KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let _ = panel.handle_paste("token123");
        assert_eq!(panel.api_key.value(), "token123");
    }

    #[test]
    fn keeps_regular_typed_input() {
        let mut panel = ConfigPanel::new(String::new(), String::new(), String::new());
        let _ = panel.handle_key_event(key('a'));
        let _ = panel.handle_key_event(key('b'));
        let _ = panel.handle_key_event(key('c'));
        assert_eq!(panel.api_key.value(), "abc");
    }
}
