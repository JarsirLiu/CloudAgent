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
}

impl ConfigPanel {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self { selected: 0, api_key, base_url, model }
    }
}

impl BottomPaneView for ConfigPanel {
    fn handle_paste(&mut self, text: &str) -> BottomPaneViewAction {
        let value = text.replace('\n', "");
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
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => self.selected = (self.selected + 1).min(3),
            KeyCode::BackTab => self.selected = self.selected.saturating_sub(1),
            KeyCode::Backspace => match self.selected {
                0 => { self.api_key.pop(); }
                1 => { self.base_url.pop(); }
                2 => { self.model.pop(); }
                _ => {}
            },
            KeyCode::Char(c) => match self.selected {
                0 => self.api_key.push(c),
                1 => self.base_url.push(c),
                2 => self.model.push(c),
                _ => {}
            },
            KeyCode::Enter => {
                if self.selected == 3 {
                    return BottomPaneViewAction::Composer(ComposerIntent::ConfigSave {
                        api_key: self.api_key.trim().to_string(),
                        base_url: self.base_url.trim().to_string(),
                        model: self.model.trim().to_string(),
                    });
                }
                self.selected = (self.selected + 1).min(3);
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
            Line::from("  Type values, Tab/Up/Down switch fields, Enter on Save"),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(if self.selected == 0 { "> API Key  : " } else { "  API Key  : " }, row_style(self.selected == 0)),
                Span::styled(if self.api_key.is_empty() { "(empty)".to_string() } else { "*".repeat(self.api_key.chars().count().min(24)) }, Style::default().fg(Color::Rgb(120, 130, 150))),
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
