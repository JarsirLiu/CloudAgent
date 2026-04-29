use agent_protocol::FrontendMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::widgets::textarea::{TextArea, display_width};

#[derive(Debug)]
pub enum ComposerAction {
    Submit(String),
    Interrupt,
    Exit,
    Reset,
    None,
}

pub struct ComposerRender {
    pub lines: Vec<Line<'static>>,
    pub cursor_row: u16,
}

pub struct ChatComposer {
    textarea: TextArea,
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            textarea: TextArea::new(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<ComposerAction> {
        if !matches!(key.kind, KeyEventKind::Press) {
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(match key.code {
                KeyCode::Char('c') | KeyCode::Char('q') => ComposerAction::Exit,
                KeyCode::Char('k') => ComposerAction::Interrupt,
                KeyCode::Char('j') => self.submit(),
                _ => {
                    self.textarea.handle_key(key);
                    ComposerAction::None
                }
            });
        }

        match key.code {
            KeyCode::Enter => Some(self.submit()),
            _ => {
                self.textarea.handle_key(key);
                None
            }
        }
    }

    pub fn render(&self, mode: FrontendMode, width: usize) -> ComposerRender {
        let (prompt_text, prompt_color, prompt_bg) = match mode {
            FrontendMode::WaitingForApproval => (
                "reply",
                Color::Rgb(255, 184, 76),
                Some(Color::Rgb(45, 36, 18)),
            ),
            FrontendMode::Running => (
                "message",
                Color::Rgb(100, 160, 255),
                Some(Color::Rgb(18, 28, 45)),
            ),
            FrontendMode::Idle => (
                "message",
                Color::Rgb(140, 140, 160),
                Some(Color::Rgb(32, 32, 40)),
            ),
        };

        let prefix = format!("  {prompt_text:<8}");
        let prefix_width = display_width(&prefix);
        let content_width = width.saturating_sub(prefix_width + 2).max(10);

        let body = if self.textarea.is_empty() {
            match mode {
                FrontendMode::Idle => "Ask anything — e.g. \"check disk pressure\"",
                FrontendMode::WaitingForApproval => "Type y / n, or enter a short reason",
                FrontendMode::Running => "",
            }
        } else {
            self.textarea.text()
        };

        let wrapped = self.textarea.wrapped_lines(body, content_width);
        let is_placeholder = self.textarea.is_empty();
        let mut lines = Vec::new();
        let cursor_row = wrapped.len().saturating_sub(1) as u16;

        for (index, wrapped_line) in wrapped.into_iter().enumerate() {
            let indent = if index == 0 {
                prefix.clone()
            } else {
                " ".repeat(prefix_width)
            };
            let prompt_style = {
                let base = Style::default()
                    .fg(prompt_color)
                    .add_modifier(Modifier::BOLD);
                if index == 0 {
                    prompt_bg.map_or(base, |bg| base.bg(bg))
                } else {
                    Style::default().fg(Color::Rgb(55, 55, 68))
                }
            };
            lines.push(Line::from(vec![
                Span::styled(indent, prompt_style),
                Span::styled(
                    wrapped_line,
                    if is_placeholder {
                        Style::default().fg(Color::Rgb(65, 65, 80))
                    } else {
                        Style::default().fg(Color::Rgb(220, 220, 230))
                    },
                ),
            ]));
        }

        ComposerRender { lines, cursor_row }
    }

    pub fn desired_height(&self, mode: FrontendMode, width: usize) -> u16 {
        self.render(mode, width).lines.len() as u16
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    pub fn cursor_position(&self, area: Rect, mode: FrontendMode) -> (u16, u16) {
        let prompt = match mode {
            FrontendMode::WaitingForApproval => "  reply   ",
            _ => "  message ",
        };
        let prompt_width = display_width(prompt);
        let available = area.width.saturating_sub(prompt_width as u16 + 2).max(1) as usize;
        let cursor_col = self.textarea.display_width_before_cursor();
        let cursor_row = (cursor_col / available) as u16;
        let cursor_col_in_row = cursor_col % available;
        let offset = if cursor_col + prompt_width >= available + prompt_width {
            (cursor_col + prompt_width).saturating_sub(available + prompt_width - 1)
        } else {
            0
        };
        let x = area.x + (prompt_width + cursor_col_in_row).saturating_sub(offset) as u16;
        let y = area.y + cursor_row;
        (x, y)
    }

    fn submit(&mut self) -> ComposerAction {
        let text = self.textarea.take_trimmed();
        if text.is_empty() {
            ComposerAction::None
        } else {
            match text.as_str() {
                "/clear" => ComposerAction::Reset,
                "/copy" => ComposerAction::Submit("/copy".to_string()),
                "/interrupt" => ComposerAction::Interrupt,
                "/exit" | "/quit" => ComposerAction::Exit,
                _ => ComposerAction::Submit(text),
            }
        }
    }
}
