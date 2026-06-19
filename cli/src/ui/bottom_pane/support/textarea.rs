use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::cell::Cell;
use unicode_segmentation::UnicodeSegmentation;

use crate::text_width::display_width;

#[derive(Debug, Clone)]
struct UndoState {
    text: String,
    cursor: usize,
    selection_anchor: Option<usize>,
}

#[derive(Debug, Default, Clone)]
pub struct TextArea {
    text: String,
    cursor: usize,
    selection_anchor: Option<usize>,
    preferred_column: Option<usize>,
    undo_stack: Vec<UndoState>,
    kill_buffer: String,
    last_wrap_width: Cell<Option<usize>>,
    elements: Vec<TextElement>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TextAreaState {
    pub scroll: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextElement {
    payload: String,
    range: std::ops::Range<usize>,
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
        self.selection_range() == Some((0, char_len(&self.text))) && !self.text.is_empty()
    }

    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        Some(self.text.chars().skip(start).take(end - start).collect())
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selection_anchor = None;
        self.preferred_column = None;
        self.undo_stack.clear();
        self.last_wrap_width.set(None);
        self.elements.clear();
    }

    pub fn set_text(&mut self, value: impl Into<String>) {
        self.text = value.into();
        self.cursor = char_len(&self.text);
        self.selection_anchor = None;
        self.preferred_column = None;
        self.undo_stack.clear();
        self.last_wrap_width.set(None);
        self.elements.clear();
    }

    pub fn set_text_clearing_elements(&mut self, value: impl Into<String>) {
        self.set_text(value);
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
                KeyCode::Char('b') => self.move_cursor_left(),
                KeyCode::Char('e') => self.move_cursor_to_line_end(),
                KeyCode::Char('f') => self.move_cursor_right(),
                KeyCode::Char('h') => self.backspace(),
                KeyCode::Char('n') => self.move_cursor_down_with_select(false),
                KeyCode::Char('p') => self.move_cursor_up_with_select(false),
                KeyCode::Left => self.move_word_left(),
                KeyCode::Right => self.move_word_right(),
                KeyCode::Char('u') => self.kill_to_line_start(),
                KeyCode::Char('k') => self.kill_to_line_end(),
                KeyCode::Char('y') => self.yank(),
                KeyCode::Char('w') => self.delete_word_before(),
                KeyCode::Char('x') => {
                    let _ = self.cut_selection();
                }
                KeyCode::Char('z') => self.undo(),
                _ => {}
            }
            return;
        }

        if key.modifiers == KeyModifiers::ALT {
            match key.code {
                KeyCode::Char('b') | KeyCode::Left => self.move_word_left(),
                KeyCode::Char('d') | KeyCode::Delete => self.delete_word_forward(),
                KeyCode::Char('f') | KeyCode::Right => self.move_word_right(),
                KeyCode::Backspace => self.delete_word_before(),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => {
                self.move_cursor_left_with_select(key.modifiers.contains(KeyModifiers::SHIFT))
            }
            KeyCode::Right => {
                self.move_cursor_right_with_select(key.modifiers.contains(KeyModifiers::SHIFT))
            }
            KeyCode::Up => {
                self.move_cursor_up_with_select(key.modifiers.contains(KeyModifiers::SHIFT))
            }
            KeyCode::Down => {
                self.move_cursor_down_with_select(key.modifiers.contains(KeyModifiers::SHIFT))
            }
            KeyCode::Home => {
                self.move_cursor_to_start_with_select(key.modifiers.contains(KeyModifiers::SHIFT))
            }
            KeyCode::End => {
                self.move_cursor_to_end_with_select(key.modifiers.contains(KeyModifiers::SHIFT))
            }
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

    pub fn desired_height(&self, width: usize) -> u16 {
        self.last_wrap_width.set(Some(width));
        wrapped_line_ranges(&self.text, width).len() as u16
    }

    pub fn visible_wrapped_lines(
        &self,
        text: &str,
        width: usize,
        viewport_height: u16,
        state: &mut TextAreaState,
    ) -> Vec<String> {
        self.last_wrap_width.set(Some(width));
        let wrapped = self.wrapped_lines(text, width);
        let scroll = self.effective_scroll(viewport_height, wrapped.len(), state.scroll, width);
        state.scroll = scroll;
        wrapped
            .into_iter()
            .skip(scroll as usize)
            .take(viewport_height as usize)
            .collect()
    }

    pub fn visual_cursor_position(&self, width: usize) -> (usize, usize) {
        if width == 0 {
            return (0, 0);
        }
        self.last_wrap_width.set(Some(width));
        let lines = wrapped_line_ranges(&self.text, width);
        let row = wrapped_line_index_by_start(&lines, self.cursor).unwrap_or(0);
        let line = lines.get(row).copied().unwrap_or((0, 0));
        let col = display_width(
            &self
                .text
                .graphemes(true)
                .skip(line.0)
                .take(self.cursor.saturating_sub(line.0))
                .collect::<String>(),
        );
        (row, col)
    }

    pub fn visual_cursor_position_with_state(
        &self,
        width: usize,
        viewport_height: u16,
        state: &mut TextAreaState,
    ) -> (usize, usize) {
        let (row, col) = self.visual_cursor_position(width);
        let scroll = self.effective_scroll(
            viewport_height,
            self.desired_height(width) as usize,
            state.scroll,
            width,
        );
        state.scroll = scroll;
        (row.saturating_sub(scroll as usize), col)
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let selected = self.selected_text()?;
        self.push_undo_state();
        self.delete_selection_only();
        Some(selected)
    }

    pub fn insert_element(&mut self, payload: &str) {
        let cursor = self.normalize_insertion_cursor(self.cursor);
        self.cursor = cursor;
        self.push_undo_state();
        self.replace_selection_if_needed();
        let start = self.cursor;
        let at = byte_index_from_char_index(&self.text, start);
        self.text.insert_str(at, payload);
        let len = char_len(payload);
        self.shift_elements_after(start, len as isize);
        self.elements.push(TextElement {
            payload: payload.to_string(),
            range: start..start + len,
        });
        self.elements.sort_by_key(|element| element.range.start);
        self.cursor = start + len;
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    pub fn element_payloads(&self) -> Vec<String> {
        self.elements
            .iter()
            .map(|element| element.payload.clone())
            .collect()
    }

    pub fn replace_element_payload(&mut self, current: &str, expected: &str) -> bool {
        let Some(index) = self
            .elements
            .iter()
            .position(|element| element.payload == current)
        else {
            return false;
        };
        let range = self.elements[index].range.clone();
        let start_byte = byte_index_from_char_index(&self.text, range.start);
        let end_byte = byte_index_from_char_index(&self.text, range.end);
        self.text.replace_range(start_byte..end_byte, expected);
        let old_len = range.end.saturating_sub(range.start);
        let new_len = char_len(expected);
        let delta = new_len as isize - old_len as isize;
        self.elements[index].payload = expected.to_string();
        self.elements[index].range = range.start..(range.start + new_len);
        for element in self.elements.iter_mut().skip(index + 1) {
            element.range.start = element.range.start.saturating_add_signed(delta);
            element.range.end = element.range.end.saturating_add_signed(delta);
        }
        if self.cursor > range.end {
            self.cursor = self.cursor.saturating_add_signed(delta);
        } else if self.cursor > range.start {
            self.cursor = range.start + new_len;
        }
        if let Some(anchor) = self.selection_anchor
            && anchor > range.end
        {
            self.selection_anchor = Some(anchor.saturating_add_signed(delta));
        }
        self.last_wrap_width.set(None);
        true
    }

    fn undo(&mut self) {
        let Some(state) = self.undo_stack.pop() else {
            return;
        };
        self.text = state.text;
        self.cursor = state.cursor.min(char_len(&self.text));
        self.selection_anchor = state
            .selection_anchor
            .map(|anchor| anchor.min(char_len(&self.text)));
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    fn push_undo_state(&mut self) {
        let snapshot = UndoState {
            text: self.text.clone(),
            cursor: self.cursor,
            selection_anchor: self.selection_anchor,
        };
        if self.undo_stack.last().is_some_and(|last| {
            last.text == snapshot.text
                && last.cursor == snapshot.cursor
                && last.selection_anchor == snapshot.selection_anchor
        }) {
            return;
        }
        self.undo_stack.push(snapshot);
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
    }

    fn insert_char(&mut self, ch: char) {
        self.cursor = self.normalize_insertion_cursor(self.cursor);
        self.push_undo_state();
        self.replace_selection_if_needed();
        let at = byte_index_from_char_index(&self.text, self.cursor);
        self.text.insert(at, ch);
        self.shift_elements_after(self.cursor, 1);
        self.cursor += 1;
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    pub fn insert_str(&mut self, value: &str) {
        self.cursor = self.normalize_insertion_cursor(self.cursor);
        self.push_undo_state();
        self.replace_selection_if_needed();
        let at = byte_index_from_char_index(&self.text, self.cursor);
        self.text.insert_str(at, value);
        let len = value.graphemes(true).count();
        self.shift_elements_after(self.cursor, len as isize);
        self.cursor += len;
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    pub fn replace_char_range(&mut self, range: std::ops::Range<usize>, value: &str) {
        self.push_undo_state();
        let (edit_start, edit_end) =
            self.expand_range_to_element_boundaries(range.start, range.end);
        let start = byte_index_from_char_index(&self.text, edit_start);
        let end = byte_index_from_char_index(&self.text, edit_end);
        self.text.replace_range(start..end, value);
        self.update_elements_for_edit(edit_start, edit_end, value.graphemes(true).count());
        self.cursor = edit_start + value.chars().count();
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    fn backspace(&mut self) {
        if self.delete_selection_if_any() {
            return;
        }
        if self.cursor == 0 {
            return;
        }
        if let Some((range, start_cursor)) = self
            .element_for_backspace(self.cursor)
            .map(|element| (element.range.clone(), element.range.start))
        {
            self.push_undo_state();
            self.delete_char_range_internal(range);
            self.cursor = start_cursor;
            self.selection_anchor = None;
            self.preferred_column = None;
            self.last_wrap_width.set(None);
            return;
        }
        self.push_undo_state();
        let start = byte_index_from_char_index(&self.text, self.cursor - 1);
        let end = byte_index_from_char_index(&self.text, self.cursor);
        self.text.replace_range(start..end, "");
        self.update_elements_for_edit(self.cursor - 1, self.cursor, 0);
        self.cursor -= 1;
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    fn delete(&mut self) {
        if self.delete_selection_if_any() {
            return;
        }
        let len = char_len(&self.text);
        if self.cursor >= len {
            return;
        }
        if let Some((range, start_cursor)) = self
            .element_for_delete(self.cursor)
            .map(|element| (element.range.clone(), element.range.start))
        {
            self.push_undo_state();
            self.delete_char_range_internal(range);
            self.cursor = start_cursor;
            self.selection_anchor = None;
            self.preferred_column = None;
            self.last_wrap_width.set(None);
            return;
        }
        self.push_undo_state();
        let start = byte_index_from_char_index(&self.text, self.cursor);
        let end = byte_index_from_char_index(&self.text, self.cursor + 1);
        self.text.replace_range(start..end, "");
        self.update_elements_for_edit(self.cursor, self.cursor + 1, 0);
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    fn delete_word_before(&mut self) {
        if self.delete_selection_if_any() {
            return;
        }
        if self.cursor == 0 {
            return;
        }
        self.push_undo_state();
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
        self.kill_buffer = self.text[start..end].to_string();
        self.text.replace_range(start..end, "");
        self.update_elements_for_edit(i, self.cursor, 0);
        self.cursor = i;
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    fn delete_selection_if_any(&mut self) -> bool {
        if self.selection_range().is_none() {
            return false;
        }
        self.push_undo_state();
        self.delete_selection_only();
        true
    }

    fn delete_selection_only(&mut self) {
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        let start_byte = byte_index_from_char_index(&self.text, start);
        let end_byte = byte_index_from_char_index(&self.text, end);
        self.text.replace_range(start_byte..end_byte, "");
        self.update_elements_for_edit(start, end, 0);
        self.cursor = start;
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    fn move_cursor_left_with_select(&mut self, selecting: bool) {
        let new_cursor = if !selecting {
            if let Some((start, _)) = self.selection_range() {
                start
            } else {
                self.cursor.saturating_sub(1)
            }
        } else {
            self.cursor.saturating_sub(1)
        };
        self.apply_cursor_move(new_cursor, selecting);
    }

    fn move_cursor_right_with_select(&mut self, selecting: bool) {
        let len = char_len(&self.text);
        let new_cursor = if !selecting {
            if let Some((_, end)) = self.selection_range() {
                end
            } else {
                self.cursor.saturating_add(1).min(len)
            }
        } else {
            self.cursor.saturating_add(1).min(len)
        };
        self.apply_cursor_move(new_cursor, selecting);
    }

    fn move_cursor_to_start_with_select(&mut self, selecting: bool) {
        self.apply_cursor_move(0, selecting);
    }

    fn move_cursor_to_end_with_select(&mut self, selecting: bool) {
        self.apply_cursor_move(char_len(&self.text), selecting);
    }

    fn move_cursor_up_with_select(&mut self, selecting: bool) {
        if !selecting && let Some(width) = self.last_wrap_width.get() {
            let lines = wrapped_line_ranges(&self.text, width);
            if let Some(idx) = wrapped_line_index_by_start(&lines, self.cursor) {
                let (start, _) = lines[idx];
                let target_col = self.preferred_column.unwrap_or_else(|| {
                    display_width(
                        &self
                            .text
                            .graphemes(true)
                            .skip(start)
                            .take(self.cursor.saturating_sub(start))
                            .collect::<String>(),
                    )
                });
                if idx > 0 {
                    let (prev_start, prev_end) = lines[idx - 1];
                    self.cursor = move_to_display_col_on_wrapped_line(
                        &self.text, prev_start, prev_end, target_col,
                    );
                    self.preferred_column = Some(target_col);
                } else {
                    self.cursor = 0;
                    self.preferred_column = None;
                }
                self.selection_anchor = None;
                return;
            }
        }
        let (line_start, current_col) = self.current_line_start_and_column();
        let target_col = self.preferred_column.unwrap_or(current_col);
        let new_cursor = if line_start == 0 {
            0
        } else {
            let previous_line_end = line_start.saturating_sub(1);
            let previous_line_start = self.line_start_before(previous_line_end);
            let previous_line_len = previous_line_end.saturating_sub(previous_line_start);
            previous_line_start + target_col.min(previous_line_len)
        };
        self.apply_cursor_move(new_cursor, selecting);
        self.preferred_column = Some(target_col);
    }

    fn move_cursor_down_with_select(&mut self, selecting: bool) {
        if !selecting && let Some(width) = self.last_wrap_width.get() {
            let lines = wrapped_line_ranges(&self.text, width);
            if let Some(idx) = wrapped_line_index_by_start(&lines, self.cursor) {
                let (start, _) = lines[idx];
                let target_col = self.preferred_column.unwrap_or_else(|| {
                    display_width(
                        &self
                            .text
                            .graphemes(true)
                            .skip(start)
                            .take(self.cursor.saturating_sub(start))
                            .collect::<String>(),
                    )
                });
                if idx + 1 < lines.len() {
                    let (next_start, next_end) = lines[idx + 1];
                    self.cursor = move_to_display_col_on_wrapped_line(
                        &self.text, next_start, next_end, target_col,
                    );
                    self.preferred_column = Some(target_col);
                } else {
                    self.cursor = char_len(&self.text);
                    self.preferred_column = None;
                }
                self.selection_anchor = None;
                return;
            }
        }
        let (line_start, current_col) = self.current_line_start_and_column();
        let target_col = self.preferred_column.unwrap_or(current_col);
        let line_end = self.line_end_after(self.cursor);
        let new_cursor = if line_end >= char_len(&self.text) {
            char_len(&self.text)
        } else {
            let next_line_start = line_end + 1;
            let next_line_end = self.line_end_after(next_line_start);
            let next_line_len = next_line_end.saturating_sub(next_line_start);
            let mut cursor = next_line_start + target_col.min(next_line_len);
            if line_start == next_line_start {
                cursor = char_len(&self.text);
            }
            cursor
        };
        self.apply_cursor_move(new_cursor, selecting);
        self.preferred_column = Some(target_col);
    }

    fn move_cursor_to_line_end(&mut self) {
        let line_end = self.line_end_after(self.cursor);
        self.apply_cursor_move(line_end, false);
    }

    fn kill_to_line_start(&mut self) {
        if self.delete_selection_if_any() {
            return;
        }
        let start = self.line_start_before(self.cursor);
        if start == self.cursor {
            return;
        }
        self.push_undo_state();
        let start_byte = byte_index_from_char_index(&self.text, start);
        let end_byte = byte_index_from_char_index(&self.text, self.cursor);
        self.kill_buffer = self.text[start_byte..end_byte].to_string();
        self.text.replace_range(start_byte..end_byte, "");
        self.update_elements_for_edit(start, self.cursor, 0);
        self.cursor = start;
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    fn kill_to_line_end(&mut self) {
        if self.delete_selection_if_any() {
            return;
        }
        let end = self.line_end_after(self.cursor);
        if end == self.cursor {
            return;
        }
        self.push_undo_state();
        let start_byte = byte_index_from_char_index(&self.text, self.cursor);
        let end_byte = byte_index_from_char_index(&self.text, end);
        self.kill_buffer = self.text[start_byte..end_byte].to_string();
        self.text.replace_range(start_byte..end_byte, "");
        self.update_elements_for_edit(self.cursor, end, 0);
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    fn yank(&mut self) {
        if self.kill_buffer.is_empty() {
            return;
        }
        let text = self.kill_buffer.clone();
        self.insert_str(&text);
    }

    fn select_all(&mut self) {
        if self.text.is_empty() {
            self.clear();
            return;
        }
        self.selection_anchor = Some(0);
        self.cursor = char_len(&self.text);
        self.preferred_column = None;
    }

    fn move_cursor_left(&mut self) {
        self.move_cursor_left_with_select(false);
    }

    fn move_cursor_right(&mut self) {
        self.move_cursor_right_with_select(false);
    }

    fn move_word_left(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = self.cursor.min(chars.len());
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        self.apply_cursor_move(i, false);
    }

    fn move_word_right(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = self.cursor.min(chars.len());
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        self.apply_cursor_move(i, false);
    }

    fn delete_word_forward(&mut self) {
        if self.delete_selection_if_any() {
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let mut end = self.cursor.min(chars.len());
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        if end == self.cursor {
            return;
        }
        self.push_undo_state();
        let start_byte = byte_index_from_char_index(&self.text, self.cursor);
        let end_byte = byte_index_from_char_index(&self.text, end);
        self.text.replace_range(start_byte..end_byte, "");
        self.update_elements_for_edit(self.cursor, end, 0);
        self.selection_anchor = None;
        self.preferred_column = None;
        self.last_wrap_width.set(None);
    }

    fn replace_selection_if_needed(&mut self) {
        if self.selection_range().is_none() {
            return;
        }
        self.delete_selection_only();
    }

    fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;
        if anchor == self.cursor {
            return None;
        }
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    fn apply_cursor_move(&mut self, new_cursor: usize, selecting: bool) {
        let len = char_len(&self.text);
        let new_cursor = self.normalize_cursor_for_move(self.cursor, new_cursor.min(len));
        if selecting {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
        } else {
            self.selection_anchor = None;
        }
        self.cursor = new_cursor;
        if !selecting {
            self.preferred_column = None;
        }
    }

    fn element_for_backspace(&self, cursor: usize) -> Option<&TextElement> {
        self.elements.iter().find(|element| {
            (element.range.start < cursor && cursor < element.range.end)
                || element.range.end == cursor
        })
    }

    fn element_for_delete(&self, cursor: usize) -> Option<&TextElement> {
        self.elements.iter().find(|element| {
            (element.range.start < cursor && cursor < element.range.end)
                || element.range.start == cursor
        })
    }

    fn normalize_insertion_cursor(&self, cursor: usize) -> usize {
        self.elements
            .iter()
            .find(|element| element.range.start < cursor && cursor < element.range.end)
            .map(|element| element.range.end)
            .unwrap_or(cursor)
    }

    fn normalize_cursor_for_move(&self, previous_cursor: usize, new_cursor: usize) -> usize {
        self.elements
            .iter()
            .find(|element| element.range.start < new_cursor && new_cursor < element.range.end)
            .map(|element| {
                if new_cursor > previous_cursor {
                    element.range.end
                } else {
                    element.range.start
                }
            })
            .unwrap_or(new_cursor)
    }

    fn shift_elements_after(&mut self, index: usize, delta: isize) {
        if delta == 0 {
            return;
        }
        for element in &mut self.elements {
            if element.range.start >= index {
                element.range.start = element.range.start.saturating_add_signed(delta);
                element.range.end = element.range.end.saturating_add_signed(delta);
            }
        }
    }

    fn expand_range_to_element_boundaries(&self, start: usize, end: usize) -> (usize, usize) {
        let mut expanded_start = start;
        let mut expanded_end = end;
        for element in &self.elements {
            let overlaps = element.range.start < expanded_end && expanded_start < element.range.end;
            if overlaps {
                expanded_start = expanded_start.min(element.range.start);
                expanded_end = expanded_end.max(element.range.end);
            }
        }
        (expanded_start, expanded_end)
    }

    fn update_elements_for_edit(&mut self, start: usize, end: usize, inserted_len: usize) {
        let delta = inserted_len as isize - (end.saturating_sub(start)) as isize;
        self.elements.retain_mut(|element| {
            if element.range.end <= start {
                true
            } else if element.range.start >= end {
                element.range.start = element.range.start.saturating_add_signed(delta);
                element.range.end = element.range.end.saturating_add_signed(delta);
                true
            } else {
                false
            }
        });
    }

    fn delete_char_range_internal(&mut self, range: std::ops::Range<usize>) {
        let start_byte = byte_index_from_char_index(&self.text, range.start);
        let end_byte = byte_index_from_char_index(&self.text, range.end);
        self.text.replace_range(start_byte..end_byte, "");
        self.update_elements_for_edit(range.start, range.end, 0);
    }

    fn current_line_start_and_column(&self) -> (usize, usize) {
        let line_start = self.line_start_before(self.cursor);
        (line_start, self.cursor.saturating_sub(line_start))
    }

    fn effective_scroll(
        &self,
        viewport_height: u16,
        total_lines: usize,
        current_scroll: u16,
        width: usize,
    ) -> u16 {
        if viewport_height == 0 || total_lines <= viewport_height as usize {
            return 0;
        }

        let (cursor_line_idx, _) = self.visual_cursor_position(width);
        let max_scroll = total_lines.saturating_sub(viewport_height as usize) as u16;
        let mut scroll = current_scroll.min(max_scroll);
        let cursor_line_idx = cursor_line_idx as u16;

        if cursor_line_idx < scroll {
            scroll = cursor_line_idx;
        } else if cursor_line_idx >= scroll.saturating_add(viewport_height) {
            scroll = cursor_line_idx
                .saturating_add(1)
                .saturating_sub(viewport_height);
        }

        scroll.min(max_scroll)
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

fn wrapped_line_index_by_start(lines: &[(usize, usize)], pos: usize) -> Option<usize> {
    let idx = lines.partition_point(|(start, _)| *start <= pos);
    if idx == 0 { None } else { Some(idx - 1) }
}

fn move_to_display_col_on_wrapped_line(
    text: &str,
    line_start: usize,
    line_end: usize,
    target_col: usize,
) -> usize {
    let graphemes: Vec<&str> = text.graphemes(true).collect();
    let mut width_so_far = 0usize;
    for (offset, grapheme) in graphemes[line_start..line_end].iter().enumerate() {
        width_so_far += display_width(grapheme);
        if width_so_far > target_col {
            return line_start + offset;
        }
    }
    line_end
}

fn wrapped_line_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
    if width == 0 || text.is_empty() {
        return vec![(0, char_len(text))];
    }

    let graphemes: Vec<&str> = text.graphemes(true).collect();
    let mut lines = Vec::new();
    let mut paragraph_start = 0usize;
    let mut idx = 0usize;

    while idx <= graphemes.len() {
        let at_end = idx == graphemes.len();
        let at_newline = !at_end && graphemes[idx] == "\n";
        if !at_end && !at_newline {
            idx += 1;
            continue;
        }

        if paragraph_start == idx {
            lines.push((paragraph_start, paragraph_start));
        } else {
            let mut line_start = paragraph_start;
            let mut line_width = 0usize;
            let mut cursor = paragraph_start;
            while cursor < idx {
                let grapheme_width = display_width(graphemes[cursor]);
                if cursor > line_start && line_width + grapheme_width > width {
                    lines.push((line_start, cursor));
                    line_start = cursor;
                    line_width = 0;
                }
                line_width += grapheme_width;
                cursor += 1;
                if line_width >= width {
                    lines.push((line_start, cursor));
                    line_start = cursor;
                    line_width = 0;
                }
            }
            if line_start < idx {
                lines.push((line_start, idx));
            }
        }

        paragraph_start = idx.saturating_add(1);
        idx = paragraph_start;
    }

    if lines.is_empty() {
        lines.push((0, 0));
    }
    lines
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
#[path = "textarea_tests.rs"]
mod tests;
