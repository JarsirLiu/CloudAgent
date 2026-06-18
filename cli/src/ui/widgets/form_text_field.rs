use crate::ui::widgets::paste_echo_guard::PasteEchoGuard;
use crate::ui::widgets::textarea::TextArea;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

pub(crate) struct FormTextField {
    textarea: TextArea,
    dirty: bool,
    paste_echo_guard: PasteEchoGuard,
}

impl FormTextField {
    pub(crate) fn new(value: String) -> Self {
        let mut textarea = TextArea::new();
        textarea.set_text(value);
        Self {
            textarea,
            dirty: false,
            paste_echo_guard: PasteEchoGuard::default(),
        }
    }

    pub(crate) fn value(&self) -> &str {
        self.textarea.text()
    }

    pub(crate) fn trimmed_value(&self) -> Option<String> {
        let trimmed = self.value().trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.value().is_empty()
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub(crate) fn cursor_display_column(&self) -> usize {
        self.textarea.visual_cursor_position(usize::MAX).1
    }

    pub(crate) fn append_paste(&mut self, text: &str) -> bool {
        let normalized = normalize_single_line_paste(text);
        if normalized.is_empty() {
            return false;
        }
        self.textarea.insert_str(&normalized);
        self.paste_echo_guard.arm(&normalized);
        self.dirty = true;
        true
    }

    pub(crate) fn append_char(&mut self, ch: char) -> bool {
        if self.paste_echo_guard.should_ignore_char(ch) {
            return false;
        }
        self.textarea.handle_key(key(KeyCode::Char(ch)));
        self.dirty = true;
        true
    }

    pub(crate) fn backspace(&mut self) {
        self.paste_echo_guard.clear();
        self.textarea.handle_key(key(KeyCode::Backspace));
        self.dirty = true;
    }

    pub(crate) fn delete(&mut self) {
        self.paste_echo_guard.clear();
        self.textarea.handle_key(key(KeyCode::Delete));
        self.dirty = true;
    }

    pub(crate) fn move_left(&mut self) {
        self.paste_echo_guard.clear();
        self.textarea.handle_key(key(KeyCode::Left));
    }

    pub(crate) fn move_right(&mut self) {
        self.paste_echo_guard.clear();
        self.textarea.handle_key(key(KeyCode::Right));
    }

    pub(crate) fn move_to_start(&mut self) {
        self.paste_echo_guard.clear();
        self.textarea.handle_key(key(KeyCode::Home));
    }

    pub(crate) fn move_to_end(&mut self) {
        self.paste_echo_guard.clear();
        self.textarea.handle_key(key(KeyCode::End));
    }

    pub(crate) fn select_all(&mut self) {
        self.paste_echo_guard.clear();
        self.textarea.handle_key(ctrl_key('a'));
    }
}

fn normalize_single_line_paste(text: &str) -> String {
    text.chars()
        .filter(|ch| *ch != '\r' && *ch != '\n')
        .collect()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn ctrl_key(ch: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(ch),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}
