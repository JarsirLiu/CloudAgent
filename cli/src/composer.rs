use agent_protocol::FrontendMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug)]
pub enum ComposerAction {
    Submit(String),
    Interrupt,
    Exit,
    History,
    Status,
    Reset,
    None,
}

pub struct Composer {
    input: String,
    cursor: usize,
}

impl Composer {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
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
                _ => ComposerAction::None,
            });
        }

        match key.code {
            KeyCode::Enter => Some(self.submit()),
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.input.remove(self.cursor);
                }
                None
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                None
            }
            KeyCode::Right => {
                self.cursor = self.cursor.saturating_add(1).min(self.input.len());
                None
            }
            KeyCode::Home => {
                self.cursor = 0;
                None
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                None
            }
            KeyCode::Delete => {
                if self.cursor < self.input.len() {
                    self.input.remove(self.cursor);
                }
                None
            }
            KeyCode::F(2) => Some(ComposerAction::History),
            KeyCode::F(3) => Some(ComposerAction::Status),
            KeyCode::F(4) => Some(ComposerAction::Reset),
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.input.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
                None
            }
            KeyCode::Tab => {
                self.input.insert_str(self.cursor, "  ");
                self.cursor += 2;
                None
            }
            _ => None,
        }
    }

    pub fn render_lines(&self, mode: FrontendMode, width: usize) -> Vec<Line<'static>> {
        let prompt = if mode == FrontendMode::WaitingForApproval {
            "approve"
        } else {
            ">"
        };
        let hint = match mode {
            FrontendMode::Idle => "? for shortcuts    Ctrl+K interrupt    F2 history    F3 status",
            FrontendMode::Running => "CloudAgent is working    Ctrl+K interrupts the current turn",
            FrontendMode::WaitingForApproval => "Type y or n, then press Enter",
        };
        let body = if self.input.trim().is_empty() && mode != FrontendMode::WaitingForApproval {
            "Try \"how does <filepath> work?\""
        } else if self.input.trim().is_empty() {
            "y or n"
        } else {
            self.input.as_str()
        };

        let content_width = width.saturating_sub(2).max(10);
        let mut rendered = Vec::new();
        for (index, line) in wrap_text(body, content_width).into_iter().enumerate() {
            rendered.push(Line::from(vec![
                Span::styled(
                    if index == 0 {
                        format!("{prompt} ")
                    } else {
                        " ".repeat(prompt.len() + 1)
                    },
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    line,
                    if self.input.trim().is_empty() && mode != FrontendMode::WaitingForApproval {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default()
                    },
                ),
            ]));
        }
        rendered.push(Line::raw(""));
        rendered.push(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::DarkGray),
        )));
        rendered
    }

    pub fn cursor_position(&self, area: Rect) -> (u16, u16) {
        let prefix = 3u16;
        let available = area.width.saturating_sub(prefix + 2) as usize;
        let offset = input_scroll_offset(self.cursor, available);
        let x = area.x + prefix + self.cursor.saturating_sub(offset) as u16;
        let y = area.y + 1;
        (x, y)
    }

    fn submit(&mut self) -> ComposerAction {
        let text = self.input.trim().to_string();
        self.input.clear();
        self.cursor = 0;
        if text.is_empty() {
            ComposerAction::None
        } else {
            ComposerAction::Submit(text)
        }
    }
}

fn input_scroll_offset(cursor: usize, width: usize) -> usize {
    if cursor < width {
        0
    } else {
        cursor - width + 1
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        let mut used = 0usize;
        for ch in paragraph.chars() {
            if used >= width {
                out.push(current);
                current = String::new();
                used = 0;
            }
            current.push(ch);
            used += 1;
        }
        if current.is_empty() {
            out.push(String::new());
        } else {
            out.push(current);
        }
    }
    out
}
