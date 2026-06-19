#[derive(Debug, Default)]
pub(super) struct ComposerHistory {
    entries: Vec<String>,
    cursor: Option<usize>,
    last_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HistoryNavigation {
    Apply(String),
    ClearComposer,
    Unchanged,
}

impl ComposerHistory {
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn clear_all(&mut self) {
        self.entries.clear();
        self.clear_navigation();
    }

    pub(super) fn clear_navigation(&mut self) {
        self.cursor = None;
        self.last_text = None;
    }

    pub(super) fn last_text_matches(&self, text: &str) -> bool {
        matches!(&self.last_text, Some(previous) if previous == text)
    }

    pub(super) fn record(&mut self, entry: String) {
        if self.entries.last().is_some_and(|last| last == &entry) {
            return;
        }
        self.entries.push(entry);
    }

    pub(super) fn navigate_up(&mut self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let next = match self.cursor {
            None => self.entries.len().saturating_sub(1),
            Some(0) => 0,
            Some(idx) => idx.saturating_sub(1),
        };
        self.cursor = Some(next);
        self.entry_at(next)
    }

    pub(super) fn navigate_down(&mut self) -> HistoryNavigation {
        let Some(idx) = self.cursor else {
            return HistoryNavigation::Unchanged;
        };
        if idx + 1 >= self.entries.len() {
            self.clear_navigation();
            return HistoryNavigation::ClearComposer;
        }
        let next = idx + 1;
        self.cursor = Some(next);
        self.entry_at(next)
            .map(HistoryNavigation::Apply)
            .unwrap_or(HistoryNavigation::Unchanged)
    }

    fn entry_at(&mut self, index: usize) -> Option<String> {
        let entry = self.entries.get(index).cloned()?;
        self.last_text = Some(entry.clone());
        Some(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::{ComposerHistory, HistoryNavigation};

    #[test]
    fn navigation_starts_from_newest_entry() {
        let mut history = ComposerHistory::default();
        history.record("first".to_string());
        history.record("second".to_string());

        assert_eq!(history.navigate_up().as_deref(), Some("second"));
        assert!(history.last_text_matches("second"));
    }

    #[test]
    fn navigation_can_move_up_and_down() {
        let mut history = ComposerHistory::default();
        history.record("first".to_string());
        history.record("second".to_string());

        assert_eq!(history.navigate_up().as_deref(), Some("second"));
        assert_eq!(history.navigate_up().as_deref(), Some("first"));
        assert_eq!(
            history.navigate_down(),
            HistoryNavigation::Apply("second".to_string())
        );
    }

    #[test]
    fn down_past_newest_clears_composer() {
        let mut history = ComposerHistory::default();
        history.record("first".to_string());

        assert_eq!(history.navigate_up().as_deref(), Some("first"));
        assert_eq!(history.navigate_down(), HistoryNavigation::ClearComposer);
        assert!(!history.last_text_matches("first"));
    }

    #[test]
    fn duplicate_consecutive_entries_are_ignored() {
        let mut history = ComposerHistory::default();
        history.record("same".to_string());
        history.record("same".to_string());

        assert_eq!(history.navigate_up().as_deref(), Some("same"));
        assert_eq!(history.navigate_down(), HistoryNavigation::ClearComposer);
    }
}
