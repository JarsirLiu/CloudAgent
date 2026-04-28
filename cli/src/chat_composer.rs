use agent_protocol::FrontendMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::textarea::{TextArea, display_width};

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

const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/history", "show session history"),
    ("/status", "show session status"),
    ("/reset", "clear current session"),
    ("/interrupt", "interrupt the active turn"),
    ("/exit", "close the cli"),
];

pub struct ComposerRender {
    pub lines: Vec<Line<'static>>,
    pub cursor_row: u16,
}

pub struct ChatComposer {
    textarea: TextArea,
    slash_selected: usize,
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            textarea: TextArea::new(),
            slash_selected: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<ComposerAction> {
        if !matches!(key.kind, KeyEventKind::Press) {
            return None;
        }

        if self.is_slash_popup_visible() {
            return Some(self.handle_slash_popup_key(key));
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
            KeyCode::F(2) => Some(ComposerAction::History),
            KeyCode::F(3) => Some(ComposerAction::Status),
            KeyCode::F(4) => Some(ComposerAction::Reset),
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
        let mut cursor_row = 0u16;

        if let Some(popup_lines) = self.render_slash_popup(width) {
            cursor_row += popup_lines.len() as u16;
            lines.extend(popup_lines);
        }

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

    pub fn cursor_position(&self, area: Rect, mode: FrontendMode) -> (u16, u16) {
        let prompt = match mode {
            FrontendMode::WaitingForApproval => "  reply   ",
            _ => "  message ",
        };
        let prompt_width = display_width(prompt);
        let available = area.width.saturating_sub(prompt_width as u16 + 2) as usize;
        let cursor_col = self.textarea.display_width_before_cursor();
        let offset = if cursor_col + prompt_width >= available + prompt_width {
            (cursor_col + prompt_width).saturating_sub(available + prompt_width - 1)
        } else {
            0
        };
        let x = area.x + (prompt_width + cursor_col).saturating_sub(offset) as u16;
        (x, area.y)
    }

    fn submit(&mut self) -> ComposerAction {
        let text = self.textarea.take_trimmed();
        self.slash_selected = 0;
        if text.is_empty() {
            ComposerAction::None
        } else {
            match text.as_str() {
                "/history" => ComposerAction::History,
                "/status" => ComposerAction::Status,
                "/reset" => ComposerAction::Reset,
                "/interrupt" => ComposerAction::Interrupt,
                "/exit" | "/quit" => ComposerAction::Exit,
                _ => ComposerAction::Submit(text),
            }
        }
    }

    fn handle_slash_popup_key(&mut self, key: KeyEvent) -> ComposerAction {
        let matches = self.matching_slash_commands();
        match key.code {
            KeyCode::Up => {
                self.slash_selected = self.slash_selected.saturating_sub(1);
                ComposerAction::None
            }
            KeyCode::Down => {
                self.slash_selected = self
                    .slash_selected
                    .saturating_add(1)
                    .min(matches.len().saturating_sub(1));
                ComposerAction::None
            }
            KeyCode::Tab => {
                if let Some((command, _)) = matches.get(self.slash_selected) {
                    self.replace_with_command(command);
                }
                ComposerAction::None
            }
            KeyCode::Enter => {
                if let Some((command, _)) = matches.get(self.slash_selected) {
                    self.replace_with_command(command);
                }
                self.submit()
            }
            KeyCode::Esc => {
                self.slash_selected = 0;
                ComposerAction::None
            }
            _ => {
                self.textarea.handle_key(key);
                self.normalize_selection();
                ComposerAction::None
            }
        }
    }

    fn is_slash_popup_visible(&self) -> bool {
        self.textarea.text().starts_with('/')
    }

    fn matching_slash_commands(&self) -> Vec<(&'static str, &'static str)> {
        let query = self.textarea.text().trim();
        let mut matches = SLASH_COMMANDS
            .iter()
            .copied()
            .filter(|(command, _)| command.starts_with(query))
            .collect::<Vec<_>>();
        if matches.is_empty() && query == "/" {
            matches = SLASH_COMMANDS.to_vec();
        }
        matches
    }

    fn normalize_selection(&mut self) {
        let len = self.matching_slash_commands().len();
        if len == 0 {
            self.slash_selected = 0;
        } else {
            self.slash_selected = self.slash_selected.min(len - 1);
        }
    }

    fn replace_with_command(&mut self, command: &str) {
        self.textarea.clear();
        for ch in command.chars() {
            self.textarea.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        self.slash_selected = 0;
    }

    fn render_slash_popup(&self, width: usize) -> Option<Vec<Line<'static>>> {
        let matches = self.matching_slash_commands();
        if matches.is_empty() {
            return None;
        }

        let desc_width = width.saturating_sub(22).max(8);
        let mut lines = vec![
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Commands",
                    Style::default()
                        .fg(Color::Rgb(205, 205, 220))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Esc close", Style::default().fg(Color::Rgb(95, 95, 110))),
            ]),
        ];

        for (index, (command, description)) in matches.iter().take(5).enumerate() {
            let selected = index == self.slash_selected;
            lines.push(Line::from(vec![
                Span::styled(
                    if selected { "  > " } else { "    " },
                    Style::default().fg(if selected {
                        Color::Rgb(255, 184, 76)
                    } else {
                        Color::Rgb(95, 95, 110)
                    }),
                ),
                Span::styled(
                    format!("{command:<12}"),
                    Style::default()
                        .fg(if selected { Color::White } else { Color::Rgb(220, 220, 230) })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    truncate(description, desc_width),
                    Style::default().fg(if selected {
                        Color::Rgb(200, 200, 215)
                    } else {
                        Color::Rgb(120, 120, 135)
                    }),
                ),
            ]));
        }
        lines.push(Line::raw(""));
        Some(lines)
    }
}

fn truncate(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width.saturating_sub(1) {
            out.push('…');
            return out;
        }
        out.push(ch);
        width += ch_width;
    }
    out
}
