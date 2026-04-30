use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use textwrap::{Options, WordSplitter, wrap};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Default, Clone)]
pub struct TextArea {
    text: String,
    cursor: usize,
}

impl TextArea {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn byte_cursor(&self) -> usize {
        byte_index_from_char_index(&self.text, self.cursor)
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn set_text(&mut self, value: impl Into<String>) {
        self.text = value.into();
        self.cursor = char_len(&self.text);
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('a') => self.cursor = 0,
                KeyCode::Char('e') => self.cursor = char_len(&self.text),
                KeyCode::Char('u') => self.delete_before_cursor(),
                KeyCode::Char('w') => self.delete_word_before(),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = self.cursor.saturating_add(1).min(char_len(&self.text)),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = char_len(&self.text),
            KeyCode::Tab => self.insert_str("  "),
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.insert_char(ch);
            }
            _ => {}
        }
    }

    pub fn take_trimmed(&mut self) -> String {
        let text = self.text.trim().to_string();
        self.clear();
        text
    }

    pub fn wrapped_lines(&self, text: &str, width: usize) -> Vec<String> {
        wrap_text(text, width)
    }

    pub fn visual_cursor_position(&self, width: usize) -> (usize, usize) {
        if width == 0 {
            return (0, 0);
        }

        let before_cursor: String = self.text.graphemes(true).take(self.cursor).collect();
        let mut row = 0usize;
        let paragraphs = before_cursor.split('\n').collect::<Vec<_>>();

        for paragraph in paragraphs.iter().take(paragraphs.len().saturating_sub(1)) {
            row += wrap_text(paragraph, width).len();
        }

        let current = paragraphs.last().copied().unwrap_or_default();
        let wrapped = wrap_text(current, width);
        row += wrapped.len().saturating_sub(1);
        let col = wrapped
            .last()
            .map(|line| UnicodeWidthStr::width(line.as_str()))
            .unwrap_or_default();
        (row, col)
    }

    fn insert_char(&mut self, ch: char) {
        let at = byte_index_from_char_index(&self.text, self.cursor);
        self.text.insert(at, ch);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, value: &str) {
        let at = byte_index_from_char_index(&self.text, self.cursor);
        self.text.insert_str(at, value);
        self.cursor += value.graphemes(true).count();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = byte_index_from_char_index(&self.text, self.cursor - 1);
        let end = byte_index_from_char_index(&self.text, self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn delete(&mut self) {
        let len = char_len(&self.text);
        if self.cursor >= len {
            return;
        }
        let start = byte_index_from_char_index(&self.text, self.cursor);
        let end = byte_index_from_char_index(&self.text, self.cursor + 1);
        self.text.replace_range(start..end, "");
    }

    fn delete_before_cursor(&mut self) {
        let byte_end = byte_index_from_char_index(&self.text, self.cursor);
        self.text.drain(..byte_end);
        self.cursor = 0;
    }

    fn delete_word_before(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = self.cursor;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        let start = byte_index_from_char_index(&self.text, i);
        let end = byte_index_from_char_index(&self.text, self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor = i;
    }
}

pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 || text.is_empty() {
        return vec![text.to_string()];
    }
    let options = Options::new(width)
        .break_words(false)
        .word_splitter(WordSplitter::NoHyphenation);

    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        for line in wrap(paragraph, &options) {
            lines.push(line.into_owned());
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn char_len(s: &str) -> usize {
    s.graphemes(true).count()
}

fn byte_index_from_char_index(s: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    s.grapheme_indices(true)
        .nth(char_index)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}
