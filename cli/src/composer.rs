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
    cursor: usize, // char index
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
                KeyCode::Char('a') => {
                    self.cursor = 0;
                    ComposerAction::None
                }
                KeyCode::Char('e') => {
                    self.cursor = char_len(&self.input);
                    ComposerAction::None
                }
                KeyCode::Char('u') => {
                    // delete from start to cursor
                    let byte_end = byte_index_from_char_index(&self.input, self.cursor);
                    self.input.drain(..byte_end);
                    self.cursor = 0;
                    ComposerAction::None
                }
                KeyCode::Char('w') => {
                    // delete word before cursor
                    self.delete_word_before();
                    ComposerAction::None
                }
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
            KeyCode::Delete => {
                let len = char_len(&self.input);
                if self.cursor < len {
                    let start = byte_index_from_char_index(&self.input, self.cursor);
                    let end = byte_index_from_char_index(&self.input, self.cursor + 1);
                    self.input.replace_range(start..end, "");
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

    /// Render the input area as ratatui Lines.
    /// Returns lines to be placed inside the bottom pane.
    pub fn render_lines(&self, mode: FrontendMode, width: usize) -> Vec<Line<'static>> {
        let is_approval = mode == FrontendMode::WaitingForApproval;
        let is_running = mode == FrontendMode::Running;

        // ── Input row ─────────────────────────────────────────────────────────
        let (prompt_text, prompt_color) = if is_approval {
            ("approval  ", Color::Rgb(255, 180, 50))
        } else if is_running {
            ("working   ", Color::Rgb(100, 160, 255))
        } else {
            ("message   ", Color::Rgb(140, 140, 160))
        };

        let prefix = format!("  {prompt_text}");
        let prefix_w = display_width(&prefix);
        let content_w = width.saturating_sub(prefix_w + 2).max(10);

        let display_body = if self.input.is_empty() {
            // Placeholder
            match mode {
                FrontendMode::Idle => "Ask anything — e.g. \"check disk pressure\"",
                FrontendMode::WaitingForApproval => "y  approve  /  n  deny",
                FrontendMode::Running => "",
            }
        } else {
            self.input.as_str()
        };

        let is_placeholder = self.input.is_empty();
        let wrapped = wrap_text(display_body, content_w);

        let mut lines: Vec<Line<'static>> = Vec::new();

        for (i, wl) in wrapped.into_iter().enumerate() {
            let indent = if i == 0 {
                prefix.clone()
            } else {
                " ".repeat(prefix_w)
            };
            lines.push(Line::from(vec![
                Span::styled(
                    indent,
                    Style::default().fg(prompt_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    wl,
                    if is_placeholder {
                        Style::default().fg(Color::Rgb(65, 65, 80))
                    } else {
                        Style::default().fg(Color::Rgb(220, 220, 230))
                    },
                ),
            ]));
        }

        // ── Hint row ─────────────────────────────────────────────────────────
        let hint = match mode {
            FrontendMode::Idle => {
                "  Enter ↵  send  ·  Ctrl+K  interrupt  ·  F2  history  ·  F4  reset"
            }
            FrontendMode::Running => "  Ctrl+K  interrupt the current turn",
            FrontendMode::WaitingForApproval => {
                "  y / n  then Enter  ·  or type a reason before approving"
            }
        };
        lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::Rgb(55, 55, 68)),
        )));

        lines
    }

    pub fn cursor_position(&self, area: Rect) -> (u16, u16) {
        // Offset: "  message   " = 12 chars
        let prefix_w = display_width("  message   ");
        let available = area.width.saturating_sub(prefix_w as u16 + 2) as usize;
        let before: String = self.input.chars().take(self.cursor).collect();
        let cursor_col = display_width(&before);
        let offset = if cursor_col + prefix_w >= available + prefix_w {
            (cursor_col + prefix_w).saturating_sub(available + prefix_w - 1)
        } else {
            0
        };
        let x = area.x + (prefix_w + cursor_col).saturating_sub(offset) as u16;
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

    fn delete_word_before(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let chars: Vec<char> = self.input.chars().collect();
        let mut i = self.cursor;
        // skip trailing spaces
        while i > 0 && chars[i - 1] == ' ' {
            i -= 1;
        }
        // skip word chars
        while i > 0 && chars[i - 1] != ' ' {
            i -= 1;
        }
        let byte_start = byte_index_from_char_index(&self.input, i);
        let byte_end = byte_index_from_char_index(&self.input, self.cursor);
        self.input.replace_range(byte_start..byte_end, "");
        self.cursor = i;
    }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn byte_index_from_char_index(s: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    s.char_indices()
        .nth(char_index)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 || text.is_empty() {
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
        for word in paragraph.split_inclusive(' ') {
            let w = display_width(word);
            if used + w > width && !current.is_empty() {
                out.push(current.trim_end().to_string());
                current = String::new();
                used = 0;
            }
            current.push_str(word);
            used += w;
        }
        if !current.is_empty() {
            out.push(current.trim_end().to_string());
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}
