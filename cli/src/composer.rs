use agent_protocol::FrontendMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

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
                    let start = byte_index_from_char_index(&self.input, self.cursor - 1);
                    let end = byte_index_from_char_index(&self.input, self.cursor);
                    self.input.replace_range(start..end, "");
                    self.cursor -= 1;
                }
                None
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                None
            }
            KeyCode::Right => {
                self.cursor = self.cursor.saturating_add(1).min(char_len(&self.input));
                None
            }
            KeyCode::Home => {
                self.cursor = 0;
                None
            }
            KeyCode::End => {
                self.cursor = char_len(&self.input);
                None
            }
            KeyCode::Delete => {
                let len = char_len(&self.input);
                if self.cursor < len {
                    let start = byte_index_from_char_index(&self.input, self.cursor);
                    let end = byte_index_from_char_index(&self.input, self.cursor + 1);
                    self.input.replace_range(start..end, "");
                }
                None
            }
            KeyCode::F(2) => Some(ComposerAction::History),
            KeyCode::F(3) => Some(ComposerAction::Status),
            KeyCode::F(4) => Some(ComposerAction::Reset),
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                let at = byte_index_from_char_index(&self.input, self.cursor);
                self.input.insert(at, ch);
                self.cursor += 1;
                None
            }
            KeyCode::Tab => {
                let at = byte_index_from_char_index(&self.input, self.cursor);
                self.input.insert_str(at, "  ");
                self.cursor += 2;
                None
            }
            _ => None,
        }
    }

    pub fn render_lines(&self, mode: FrontendMode, width: usize) -> Vec<Line<'static>> {
        let prompt = if mode == FrontendMode::WaitingForApproval {
            "choice"
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
            "y / n"
        } else {
            self.input.as_str()
        };

        let prefix = format!("{prompt} ");
        let content_width = width.saturating_sub(display_width(&prefix)).max(10);
        let mut rendered = Vec::new();
        for (index, line) in wrap_text(body, content_width).into_iter().enumerate() {
            rendered.push(Line::from(vec![
                Span::styled(
                    if index == 0 {
                        prefix.clone()
                    } else {
                        " ".repeat(display_width(&prefix))
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
        let prefix = "> ";
        let prefix_width = display_width(prefix);
        let available = area.width.saturating_sub(prefix_width as u16 + 1) as usize;
        let before_cursor =
            prefix.to_string() + &self.input.chars().take(self.cursor).collect::<String>();
        let line_width = display_width(&before_cursor);
        let offset = input_scroll_offset(line_width, available);
        let x = area.x + line_width.saturating_sub(offset) as u16;
        let y = area.y;
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

fn input_scroll_offset(cursor_width: usize, width: usize) -> usize {
    if cursor_width < width {
        0
    } else {
        cursor_width - width + 1
    }
}

fn char_len(value: &str) -> usize {
    value.chars().count()
}

fn byte_index_from_char_index(value: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    value
        .char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(value.len())
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut used = 0usize;
        for ch in paragraph.chars() {
            let ch_width = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
            if used + ch_width > width && !current.is_empty() {
                out.push(current);
                current = String::new();
                used = 0;
            }
            current.push(ch);
            used += ch_width.max(1);
        }
        if current.is_empty() {
            out.push(String::new());
        } else {
            out.push(current);
        }
    }
    out
}
