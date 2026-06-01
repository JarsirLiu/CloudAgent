use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscriptScrollIntent {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptScroll {
    top_row: usize,
    follow_tail: bool,
    content_rows: usize,
    viewport_rows: usize,
}

impl TranscriptScroll {
    pub(crate) fn reset(&mut self) {
        *self = Self {
            follow_tail: true,
            ..Self::default()
        };
    }

    pub(crate) fn top_row_for_render(&mut self, content_rows: usize, viewport_rows: usize) -> u16 {
        self.content_rows = content_rows;
        self.viewport_rows = viewport_rows;
        let max_top = self.max_top();
        if self.follow_tail {
            self.top_row = max_top;
        } else {
            self.top_row = self.top_row.min(max_top);
            if self.top_row == max_top {
                self.follow_tail = true;
            }
        }
        self.top_row.min(u16::MAX as usize) as u16
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> bool {
        let Some(intent) = intent_from_key(key) else {
            return false;
        };
        self.apply_intent(intent)
    }

    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> bool {
        let Some(intent) = intent_from_mouse(mouse) else {
            return false;
        };
        self.apply_intent(intent)
    }

    pub(crate) fn is_at_top(&self) -> bool {
        self.top_row == 0
    }

    fn apply_intent(&mut self, intent: TranscriptScrollIntent) -> bool {
        if self.content_rows <= self.viewport_rows {
            self.follow_tail = true;
            self.top_row = 0;
            return false;
        }

        let page = self.viewport_rows.saturating_sub(1).max(1);
        match intent {
            TranscriptScrollIntent::LineUp => self.scroll_up(1),
            TranscriptScrollIntent::LineDown => self.scroll_down(1),
            TranscriptScrollIntent::PageUp => self.scroll_up(page),
            TranscriptScrollIntent::PageDown => self.scroll_down(page),
            TranscriptScrollIntent::Top => {
                self.follow_tail = false;
                self.top_row = 0;
            }
            TranscriptScrollIntent::Bottom => {
                self.follow_tail = true;
                self.top_row = self.max_top();
            }
        }
        true
    }

    fn scroll_up(&mut self, rows: usize) {
        self.follow_tail = false;
        self.top_row = self.top_row.saturating_sub(rows);
    }

    fn scroll_down(&mut self, rows: usize) {
        let max_top = self.max_top();
        self.top_row = self.top_row.saturating_add(rows).min(max_top);
        self.follow_tail = self.top_row == max_top;
    }

    fn max_top(&self) -> usize {
        self.content_rows.saturating_sub(self.viewport_rows)
    }
}

impl Default for TranscriptScroll {
    fn default() -> Self {
        Self {
            top_row: 0,
            follow_tail: true,
            content_rows: 0,
            viewport_rows: 0,
        }
    }
}

fn intent_from_key(key: KeyEvent) -> Option<TranscriptScrollIntent> {
    match key.code {
        KeyCode::Up => Some(TranscriptScrollIntent::LineUp),
        KeyCode::Down => Some(TranscriptScrollIntent::LineDown),
        KeyCode::PageUp => Some(TranscriptScrollIntent::PageUp),
        KeyCode::PageDown => Some(TranscriptScrollIntent::PageDown),
        KeyCode::Home => Some(TranscriptScrollIntent::Top),
        KeyCode::End => Some(TranscriptScrollIntent::Bottom),
        _ => None,
    }
}

fn intent_from_mouse(mouse: MouseEvent) -> Option<TranscriptScrollIntent> {
    match mouse.kind {
        MouseEventKind::ScrollUp => Some(TranscriptScrollIntent::LineUp),
        MouseEventKind::ScrollDown => Some(TranscriptScrollIntent::LineDown),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::TranscriptScroll;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn follows_tail_by_default_as_content_grows() {
        let mut scroll = TranscriptScroll::default();

        assert_eq!(scroll.top_row_for_render(20, 5), 15);
        assert_eq!(scroll.top_row_for_render(25, 5), 20);
    }

    #[test]
    fn manual_scroll_keeps_position_when_content_grows() {
        let mut scroll = TranscriptScroll::default();
        assert_eq!(scroll.top_row_for_render(20, 5), 15);

        assert!(scroll.handle_key(key(KeyCode::PageUp)));
        assert_eq!(scroll.top_row_for_render(20, 5), 11);
        assert_eq!(scroll.top_row_for_render(25, 5), 11);
    }

    #[test]
    fn end_restores_tail_following() {
        let mut scroll = TranscriptScroll::default();
        assert_eq!(scroll.top_row_for_render(20, 5), 15);
        assert!(scroll.handle_key(key(KeyCode::PageUp)));
        assert_eq!(scroll.top_row_for_render(30, 5), 11);

        assert!(scroll.handle_key(key(KeyCode::End)));
        assert_eq!(scroll.top_row_for_render(30, 5), 25);
        assert_eq!(scroll.top_row_for_render(35, 5), 30);
    }
}
