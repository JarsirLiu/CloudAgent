use crate::input::completion::CompletionState;
use crate::input::intent::ComposerIntent;
use crate::input::slash_command::{SlashCommand, find_slash_command};
use crate::ui::widgets::completion_popup::completion_popup_lines;
use agent_protocol::FrontendMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::time::{Duration, Instant};

use crate::ui::widgets::textarea::{TextArea, display_width, is_altgr};

pub struct ComposerRender {
    pub lines: Vec<Line<'static>>,
    pub completion_lines: Vec<Line<'static>>,
    pub cursor_row: u16,
}

pub struct ChatComposer {
    textarea: TextArea,
    completion: CompletionState,
    paste_burst: PasteBurst,
}

#[derive(Debug, Default, Clone)]
struct PasteBurst {
    last_plain_key_at: Option<Instant>,
    consecutive_plain_keys: u16,
    in_paste_burst: bool,
}

impl PasteBurst {
    #[cfg(windows)]
    const CHAR_INTERVAL: Duration = Duration::from_millis(30);
    #[cfg(not(windows))]
    const CHAR_INTERVAL: Duration = Duration::from_millis(12);

    #[cfg(windows)]
    const IDLE_TIMEOUT: Duration = Duration::from_millis(70);
    #[cfg(not(windows))]
    const IDLE_TIMEOUT: Duration = Duration::from_millis(30);

    const MIN_BURST_KEYS: u16 = 3;

    fn on_plain_key(&mut self, now: Instant) {
        match self.last_plain_key_at {
            Some(prev) if now.duration_since(prev) <= Self::CHAR_INTERVAL => {
                self.consecutive_plain_keys = self.consecutive_plain_keys.saturating_add(1);
            }
            _ => {
                self.consecutive_plain_keys = 1;
                self.in_paste_burst = false;
            }
        }
        self.last_plain_key_at = Some(now);
        if self.consecutive_plain_keys >= Self::MIN_BURST_KEYS {
            self.in_paste_burst = true;
        }
    }

    fn refresh_timeout(&mut self, now: Instant) {
        if self.in_paste_burst
            && let Some(prev) = self.last_plain_key_at
            && now.duration_since(prev) > Self::IDLE_TIMEOUT
        {
            self.in_paste_burst = false;
            self.consecutive_plain_keys = 0;
            self.last_plain_key_at = None;
        }
    }

    fn reset(&mut self) {
        self.in_paste_burst = false;
        self.consecutive_plain_keys = 0;
        self.last_plain_key_at = None;
    }
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            textarea: TextArea::new(),
            completion: CompletionState::default(),
            paste_burst: PasteBurst::default(),
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<ComposerIntent> {
        if !matches!(key.kind, KeyEventKind::Press) {
            return None;
        }
        let now = Instant::now();
        self.paste_burst.refresh_timeout(now);
        let is_plain_char = matches!(key.code, KeyCode::Char(_))
            && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT);
        let is_plain_enter = key.code == KeyCode::Enter && key.modifiers.is_empty();
        if is_plain_char || is_plain_enter {
            self.paste_burst.on_plain_key(now);
        } else {
            self.paste_burst.reset();
        }

        if is_plain_enter && self.paste_burst.in_paste_burst {
            self.textarea.insert_str("\n");
            self.sync_completion();
            return None;
        }

        if key.code == KeyCode::Enter && is_newline_shortcut(key.modifiers) {
            self.textarea.insert_str("\n");
            self.sync_completion();
            return None;
        }

        if key.modifiers == KeyModifiers::CONTROL {
            return Some(match key.code {
                KeyCode::Char('c') => ComposerIntent::Interrupt,
                KeyCode::Char('q') => ComposerIntent::Exit,
                KeyCode::Char('j') => self.submit(),
                _ => {
                    self.textarea.handle_key(key);
                    self.sync_completion();
                    ComposerIntent::None
                }
            });
        }

        if is_altgr(key.modifiers) {
            self.textarea.handle_key(key);
            self.sync_completion();
            return None;
        }

        if self.completion.is_active() {
            match key.code {
                KeyCode::Up => {
                    self.completion.move_up();
                    return Some(ComposerIntent::None);
                }
                KeyCode::Down => {
                    self.completion.move_down();
                    return Some(ComposerIntent::None);
                }
                KeyCode::Esc => {
                    self.completion.clear();
                    return Some(ComposerIntent::None);
                }
                KeyCode::Tab => {
                    self.accept_selected_completion();
                    return Some(ComposerIntent::None);
                }
                KeyCode::Enter => {
                    if let Some(selected) = self.completion.selected() {
                        if let Some(command) = selected.command {
                            self.textarea.clear();
                            self.completion.clear();
                            return Some(action_for_command(command, ""));
                        }
                        self.accept_selected_completion();
                        return Some(ComposerIntent::None);
                    }
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Enter => Some(self.submit()),
            _ => {
                self.textarea.handle_key(key);
                self.sync_completion();
                None
            }
        }
    }

    pub(crate) fn handle_paste(&mut self, text: &str) -> ComposerIntent {
        self.paste_burst.reset();
        self.textarea.insert_str(text);
        self.sync_completion();
        ComposerIntent::None
    }

    pub fn render(&self, mode: FrontendMode, width: usize) -> ComposerRender {
        let (prompt_text, prompt_color, prompt_bg) = match mode {
            FrontendMode::WaitingForServerRequest => ("?", Color::Rgb(255, 184, 76), None),
            FrontendMode::Running => (">", Color::Rgb(100, 160, 255), None),
            FrontendMode::Idle => (">", Color::Rgb(150, 180, 255), None),
        };

        let prefix = format!("  {prompt_text} ");
        let prefix_width = display_width(&prefix);
        let content_width = width.saturating_sub(prefix_width + 2).max(10);

        let body = if self.textarea.is_empty() {
            match mode {
                FrontendMode::Idle => "Ask anything — e.g. \"check disk pressure\"",
                FrontendMode::WaitingForServerRequest => "Type y / n, or enter a short reason",
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
        let completion_lines = completion_popup_lines(&self.completion, width, prefix_width);

        ComposerRender {
            lines,
            completion_lines,
            cursor_row,
        }
    }

    pub fn desired_height(&self, mode: FrontendMode, width: usize) -> u16 {
        let rendered = self.render(mode, width);
        rendered.lines.len() as u16
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    pub fn has_completion_menu(&self) -> bool {
        self.completion.is_active()
    }

    pub fn cursor_position(&self, area: Rect, mode: FrontendMode) -> (u16, u16) {
        let prompt = match mode {
            FrontendMode::WaitingForServerRequest => "  ? ",
            _ => "  > ",
        };
        let prompt_width = display_width(prompt);
        let composer_width = area.width as usize;
        let content_width = composer_width.saturating_sub(prompt_width + 2).max(10);
        let (cursor_row, cursor_col) = self.textarea.visual_cursor_position(content_width);
        let max_x_offset = area.width.saturating_sub(1) as usize;
        let x = area.x + (prompt_width + cursor_col).min(max_x_offset) as u16;
        let y = area.y + cursor_row as u16;
        (x, y)
    }

    fn submit(&mut self) -> ComposerIntent {
        let raw = self.textarea.text().to_string();
        let leading_space_escape = raw.starts_with(' ');
        let text = self.textarea.take_trimmed();
        self.completion.clear();
        if text.is_empty() {
            ComposerIntent::None
        } else {
            if !leading_space_escape && let Some(command_text) = text.strip_prefix('/') {
                let mut parts = command_text.splitn(2, char::is_whitespace);
                let name = parts.next().unwrap_or_default();
                let args = parts.next().unwrap_or_default().trim();
                if let Some(command) = find_slash_command(name) {
                    if args.is_empty() || command.supports_inline_args() {
                        return action_for_command(command, args);
                    }
                }
                return ComposerIntent::UnknownCommand(name.to_string());
            }
            ComposerIntent::Submit(text)
        }
    }

    fn sync_completion(&mut self) {
        self.completion
            .sync_from_input(self.textarea.text(), self.textarea.byte_cursor());
    }

    fn accept_selected_completion(&mut self) {
        let Some(selected) = self.completion.selected() else {
            return;
        };
        if let Some(command) = selected.command {
            self.textarea.set_text(format!("/{} ", command.name()));
        } else if self.textarea.text().starts_with("/filter") {
            self.textarea
                .set_text(format!("/filter {} ", selected.insertion));
        }
        self.completion.clear();
    }
}

fn is_newline_shortcut(modifiers: KeyModifiers) -> bool {
    let shift_only = modifiers.contains(KeyModifiers::SHIFT)
        && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    shift_only || modifiers == KeyModifiers::ALT || modifiers == KeyModifiers::CONTROL
}

fn action_for_command(command: SlashCommand, args: &str) -> ComposerIntent {
    match command {
        SlashCommand::Clear => ComposerIntent::Reset,
        SlashCommand::Compact => ComposerIntent::Compact,
        SlashCommand::Copy => ComposerIntent::Copy,
        SlashCommand::Help => ComposerIntent::Help,
        SlashCommand::Interrupt => ComposerIntent::Interrupt,
        SlashCommand::Session => {
            let trimmed = args.trim();
            if trimmed.is_empty() {
                ComposerIntent::Session
            } else {
                ComposerIntent::SessionSwitch(trimmed.to_string())
            }
        }
        SlashCommand::NewConversation => ComposerIntent::NewConversation(args.trim().to_string()),
        SlashCommand::SetTitle => ComposerIntent::SetTitle(args.trim().to_string()),
        SlashCommand::ArchiveConversation => {
            ComposerIntent::ArchiveConversation(args.trim().to_string())
        }
        SlashCommand::DeleteConversation => {
            ComposerIntent::DeleteConversation(args.trim().to_string())
        }
        SlashCommand::Filter => ComposerIntent::Filter(args.trim().to_string()),
        SlashCommand::Permissions => ComposerIntent::Permissions(args.trim().to_string()),
        SlashCommand::Exit => ComposerIntent::Exit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_text(composer: &mut ChatComposer, text: &str) {
        for ch in text.chars() {
            composer.handle_key(key(KeyCode::Char(ch)));
        }
    }

    #[test]
    fn slash_opens_completion_and_tab_completes() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "/co");

        assert!(composer.completion.is_active());
        composer.handle_key(key(KeyCode::Tab));
        assert_eq!(composer.textarea.text(), "/copy ");
    }

    #[test]
    fn enter_dispatches_selected_completion() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "/co");

        let action = composer.handle_key(key(KeyCode::Enter));
        assert_eq!(action, Some(ComposerIntent::Copy));
        assert!(composer.textarea.is_empty());
    }

    #[test]
    fn exact_slash_command_dispatches_without_reducer_parsing() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "/clear");

        let action = composer.handle_key(key(KeyCode::Enter));
        assert_eq!(action, Some(ComposerIntent::Reset));
    }

    #[test]
    fn leading_space_slash_submits_as_message() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, " /clear");

        let action = composer.handle_key(key(KeyCode::Enter));
        assert_eq!(action, Some(ComposerIntent::Submit("/clear".to_string())));
    }

    #[test]
    fn completion_popup_does_not_shift_cursor() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "/co");

        let (_, y) = composer.cursor_position(Rect::new(0, 10, 80, 5), FrontendMode::Idle);
        assert_eq!(y, 10);
    }

    #[test]
    fn completion_popup_scrolls_to_selected_command() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "/");
        for _ in 0..4 {
            composer.handle_key(key(KeyCode::Down));
        }

        let rendered = composer.render(FrontendMode::Idle, 80);
        let visible_text = rendered
            .completion_lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(visible_text.contains("> /"));
    }

    #[test]
    fn bracketed_paste_inserts_text_without_submitting() {
        let mut composer = ChatComposer::new();

        let action = composer.handle_paste("first line\nsecond line");

        assert_eq!(action, ComposerIntent::None);
        assert_eq!(composer.textarea.text(), "first line\nsecond line");
    }

    #[test]
    fn shift_enter_inserts_newline_without_submitting() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "first");

        let action = composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        type_text(&mut composer, "second");

        assert_eq!(action, None);
        assert_eq!(composer.textarea.text(), "first\nsecond");
    }

    #[test]
    fn alt_enter_inserts_newline_without_submitting() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "first");

        let action = composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        type_text(&mut composer, "second");

        assert_eq!(action, None);
        assert_eq!(composer.textarea.text(), "first\nsecond");
    }

    #[test]
    fn ctrl_enter_inserts_newline_without_submitting() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "first");

        let action = composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        type_text(&mut composer, "second");

        assert_eq!(action, None);
        assert_eq!(composer.textarea.text(), "first\nsecond");
    }

    #[test]
    fn rapid_key_stream_treats_enter_as_newline_not_submit() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "abc");

        let action = composer.handle_key(key(KeyCode::Enter));

        assert_eq!(action, None);
        assert_eq!(composer.textarea.text(), "abc\n");
    }

    #[test]
    fn paste_burst_times_out_then_enter_submits_again() {
        let mut burst = PasteBurst::default();
        let now = Instant::now();
        burst.on_plain_key(now);
        burst.on_plain_key(now + Duration::from_millis(1));
        burst.on_plain_key(now + Duration::from_millis(2));
        assert!(burst.in_paste_burst);

        burst.refresh_timeout(now + Duration::from_millis(200));
        assert!(!burst.in_paste_burst);
    }
}
