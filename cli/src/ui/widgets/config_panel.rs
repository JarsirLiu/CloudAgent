use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct ConfigPanel {
    selected: usize,
    api_key: String,
    base_url: String,
    model: String,
    replace_on_next_input: bool,
}

impl ConfigPanel {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self { selected: 0, api_key, base_url, model, replace_on_next_input: true }
    }

    fn field_mut(&mut self) -> Option<&mut String> {
        match self.selected {
            0 => Some(&mut self.api_key),
            1 => Some(&mut self.base_url),
            2 => Some(&mut self.model),
            _ => None,
        }
    }

    fn maybe_replace_field(&mut self) {
        if self.replace_on_next_input && let Some(field) = self.field_mut() {
            field.clear();
            self.replace_on_next_input = false;
        }
    }

    fn move_selection(&mut self, next: usize) {
        let clamped = next.min(3);
        if clamped != self.selected {
            self.selected = clamped;
            self.replace_on_next_input = true;
        }
    }
}

impl BottomPaneView for ConfigPanel {
    fn handle_paste(&mut self, text: &str) -> BottomPaneViewAction {
        let value = text.replace('\n', "");
        self.maybe_replace_field();
        match self.selected {
            0 => self.api_key.push_str(&value),
            1 => self.base_url.push_str(&value),
            2 => self.model.push_str(&value),
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
            KeyCode::Backspace => match self.selected {
                0 => {
                    if self.replace_on_next_input {
                        self.api_key.clear();
                        self.replace_on_next_input = false;
                    }
                    self.api_key.pop();
                }
                1 => {
                    if self.replace_on_next_input {
                        self.base_url.clear();
                        self.replace_on_next_input = false;
                    }
                    self.base_url.pop();
                }
                2 => {
                    if self.replace_on_next_input {
                        self.model.clear();
                        self.replace_on_next_input = false;
                    }
                    self.model.pop();
                }
                _ => {}
            },
            KeyCode::Char(c) => {
                self.maybe_replace_field();
                match self.selected {
                    0 => self.api_key.push(c),
                    1 => self.base_url.push(c),
                    2 => self.model.push(c),
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
                Style::default().fg(Color::Rgb(190, 220, 255)).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(140, 150, 180))
            }
        };
        vec![
            Line::from("  Config Panel"),
            Line::from("  Type values, Tab/Up/Down switch fields, paste directly, Enter on Save"),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(if self.selected == 0 { "> API Key  : " } else { "  API Key  : " }, row_style(self.selected == 0)),
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
                Span::styled(if self.selected == 1 { "> Base URL : " } else { "  Base URL : " }, row_style(self.selected == 1)),
                Span::styled(if self.base_url.is_empty() { "(empty)".to_string() } else { self.base_url.clone() }, Style::default().fg(Color::Rgb(210, 215, 225))),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(if self.selected == 2 { "> Model    : " } else { "  Model    : " }, row_style(self.selected == 2)),
                Span::styled(if self.model.is_empty() { "(empty)".to_string() } else { self.model.clone() }, Style::default().fg(Color::Rgb(210, 215, 225))),
            ]),
            Line::from(Span::styled(
                if self.selected == 3 { "  > [ Save ]" } else { "    [ Save ]" },
                row_style(self.selected == 3),
            )),
        ]
    }

    fn cursor_position(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}
