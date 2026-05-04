use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Default, Clone)]
pub struct TextArea {
    text: String,
    cursor: usize,
    selection_all: bool,
    preferred_column: Option<usize>,
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

    pub fn is_all_selected(&self) -> bool {
        self.selection_all && !self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selection_all = false;
        self.preferred_column = None;
    }

    pub fn set_text(&mut self, value: impl Into<String>) {
        self.text = value.into();
        self.cursor = char_len(&self.text);
        self.selection_all = false;
        self.preferred_column = None;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if let KeyEvent {
            code: KeyCode::Char(ch),
            modifiers,
            ..
        } = key
            && is_altgr(modifiers)
        {
            self.insert_char(ch);
            return;
        }

        if key.modifiers == KeyModifiers::CONTROL {
            match key.code {
                KeyCode::Char('a') => self.select_all(),
                KeyCode::Char('e') => self.move_cursor_to_end(),
                KeyCode::Char('u') => self.delete_before_cursor(),
                KeyCode::Char('w') => self.delete_word_before(),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.move_cursor_left(),
            KeyCode::Right => self.move_cursor_right(),
            KeyCode::Up => self.move_cursor_up(),
            KeyCode::Down => self.move_cursor_down(),
            KeyCode::Home => self.move_cursor_to_start(),
            KeyCode::End => self.move_cursor_to_end(),
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
        self.replace_selection_if_needed();
        let at = byte_index_from_char_index(&self.text, self.cursor);
        self.text.insert(at, ch);
        self.cursor += 1;
        self.preferred_column = None;
    }

    pub fn insert_str(&mut self, value: &str) {
        self.replace_selection_if_needed();
        let at = byte_index_from_char_index(&self.text, self.cursor);
        self.text.insert_str(at, value);
        self.cursor += value.graphemes(true).count();
        self.preferred_column = None;
    }

    fn backspace(&mut self) {
        if self.selection_all {
            self.clear();
            return;
        }
        if self.cursor == 0 {
            return;
        }
        let start = byte_index_from_char_index(&self.text, self.cursor - 1);
        let end = byte_index_from_char_index(&self.text, self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
        self.preferred_column = None;
    }

    fn delete(&mut self) {
        if self.selection_all {
            self.clear();
            return;
        }
        let len = char_len(&self.text);
        if self.cursor >= len {
            return;
        }
        let start = byte_index_from_char_index(&self.text, self.cursor);
        let end = byte_index_from_char_index(&self.text, self.cursor + 1);
        self.text.replace_range(start..end, "");
        self.preferred_column = None;
    }

    fn delete_before_cursor(&mut self) {
        if self.selection_all {
            self.clear();
            return;
        }
        let byte_end = byte_index_from_char_index(&self.text, self.cursor);
        self.text.drain(..byte_end);
        self.cursor = 0;
        self.preferred_column = None;
    }

    fn delete_word_before(&mut self) {
        if self.selection_all {
            self.clear();
            return;
        }
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
        self.preferred_column = None;
    }

    fn move_cursor_left(&mut self) {
        self.selection_all = false;
        self.cursor = self.cursor.saturating_sub(1);
        self.preferred_column = None;
    }

    fn move_cursor_right(&mut self) {
        self.selection_all = false;
        self.cursor = self.cursor.saturating_add(1).min(char_len(&self.text));
        self.preferred_column = None;
    }

    fn move_cursor_to_start(&mut self) {
        self.selection_all = false;
        self.cursor = 0;
        self.preferred_column = None;
    }

    fn move_cursor_to_end(&mut self) {
        self.selection_all = false;
        self.cursor = char_len(&self.text);
        self.preferred_column = None;
    }

    fn move_cursor_up(&mut self) {
        self.selection_all = false;
        let (line_start, current_col) = self.current_line_start_and_column();
        let target_col = self.preferred_column.unwrap_or(current_col);
        if line_start == 0 {
            self.cursor = 0;
            self.preferred_column = Some(target_col);
            return;
        }
        let previous_line_end = line_start.saturating_sub(1);
        let previous_line_start = self.line_start_before(previous_line_end);
        let previous_line_len = previous_line_end.saturating_sub(previous_line_start);
        self.cursor = previous_line_start + target_col.min(previous_line_len);
        self.preferred_column = Some(target_col);
    }

    fn move_cursor_down(&mut self) {
        self.selection_all = false;
        let (line_start, current_col) = self.current_line_start_and_column();
        let target_col = self.preferred_column.unwrap_or(current_col);
        let line_end = self.line_end_after(self.cursor);
        if line_end >= char_len(&self.text) {
            self.cursor = char_len(&self.text);
            self.preferred_column = Some(target_col);
            return;
        }
        let next_line_start = line_end + 1;
        let next_line_end = self.line_end_after(next_line_start);
        let next_line_len = next_line_end.saturating_sub(next_line_start);
        self.cursor = next_line_start + target_col.min(next_line_len);
        self.preferred_column = Some(target_col);
        if line_start == next_line_start {
            self.cursor = char_len(&self.text);
        }
    }

    fn select_all(&mut self) {
        if self.text.is_empty() {
            self.clear();
            return;
        }
        self.selection_all = true;
        self.cursor = char_len(&self.text);
        self.preferred_column = None;
    }

    fn replace_selection_if_needed(&mut self) {
        if !self.selection_all {
            return;
        }
        self.text.clear();
        self.cursor = 0;
        self.selection_all = false;
        self.preferred_column = None;
    }

    fn current_line_start_and_column(&self) -> (usize, usize) {
        let line_start = self.line_start_before(self.cursor);
        (line_start, self.cursor.saturating_sub(line_start))
    }

    fn line_start_before(&self, cursor: usize) -> usize {
        if cursor == 0 {
            return 0;
        }
        let graphemes: Vec<&str> = self.text.graphemes(true).collect();
        let mut idx = cursor.min(graphemes.len());
        while idx > 0 {
            if graphemes[idx - 1] == "\n" {
                return idx;
            }
            idx -= 1;
        }
        0
    }

    fn line_end_after(&self, cursor: usize) -> usize {
        let graphemes: Vec<&str> = self.text.graphemes(true).collect();
        let mut idx = cursor.min(graphemes.len());
        while idx < graphemes.len() {
            if graphemes[idx] == "\n" {
                return idx;
            }
            idx += 1;
        }
        graphemes.len()
    }
}

pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 || text.is_empty() {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        wrap_paragraph_preserving_spaces(paragraph, width, &mut lines);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn wrap_paragraph_preserving_spaces(paragraph: &str, width: usize, out: &mut Vec<String>) {
    let mut line = String::new();
    let mut line_width = 0usize;

    for grapheme in paragraph.graphemes(true) {
        let grapheme_width = display_width(grapheme);

        if !line.is_empty() && line_width + grapheme_width > width {
            out.push(std::mem::take(&mut line));
            line_width = 0;
        }

        line.push_str(grapheme);
        line_width += grapheme_width;

        if line_width >= width {
            out.push(std::mem::take(&mut line));
            line_width = 0;
        }
    }

    if !line.is_empty() {
        out.push(line);
    }
}

#[cfg(windows)]
pub fn is_altgr(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::ALT) && modifiers.contains(KeyModifiers::CONTROL)
}

#[cfg(not(windows))]
pub fn is_altgr(_modifiers: KeyModifiers) -> bool {
    false
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

#[cfg(test)]
mod tests {
    use super::wrap_text;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::TextArea;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn wrap_text_preserves_trailing_space_visibility() {
        assert_eq!(wrap_text("abc ", 10), vec!["abc "]);
        assert_eq!(wrap_text("abc ", 3), vec!["abc", " "]);
    }

    #[test]
    fn wrap_text_preserves_consecutive_spaces() {
        assert_eq!(wrap_text("a  b", 10), vec!["a  b"]);
        assert_eq!(wrap_text("a  b", 2), vec!["a ", " b"]);
    }

    #[test]
    fn select_all_replaces_text_on_insert() {
        let mut ta = TextArea::new();
        ta.insert_str("hello");
        ta.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        ta.handle_key(key(KeyCode::Char('x')));

        assert_eq!(ta.text(), "x");
        assert_eq!(ta.cursor(), 1);
        assert!(!ta.is_all_selected());
    }

    #[test]
    fn down_moves_to_next_line_or_text_end() {
        let mut ta = TextArea::new();
        ta.set_text("first\nsecond\nthird");
        ta.handle_key(key(KeyCode::Home));
        ta.handle_key(key(KeyCode::Down));

        assert_eq!(
            ta.text().chars().take(ta.cursor()).collect::<String>(),
            "first\n"
        );

        ta.handle_key(key(KeyCode::Down));
        ta.handle_key(key(KeyCode::Down));
        assert_eq!(ta.cursor(), ta.text().chars().count());
    }
}
