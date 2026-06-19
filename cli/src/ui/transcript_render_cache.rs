use crate::ui::history_cell::HistoryCell;
use ratatui::text::Line;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TranscriptRenderCacheKey {
    pub(crate) live_revision: u64,
    pub(crate) width: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TranscriptRenderCache {
    key: Option<TranscriptRenderCacheKey>,
    lines: Vec<Line<'static>>,
    rendered_rows: usize,
}

impl TranscriptRenderCache {
    pub(crate) fn clear(&mut self) {
        self.key = None;
        self.lines.clear();
        self.rendered_rows = 0;
    }

    pub(crate) fn is_fresh(&self, key: TranscriptRenderCacheKey) -> bool {
        self.key == Some(key)
    }

    pub(crate) fn store(
        &mut self,
        key: TranscriptRenderCacheKey,
        lines: Vec<Line<'static>>,
        rendered_rows: usize,
    ) {
        self.key = Some(key);
        self.lines = lines;
        self.rendered_rows = rendered_rows;
    }

    pub(crate) fn lines(&self) -> &[Line<'static>] {
        &self.lines
    }

    pub(crate) fn rendered_rows(&self) -> usize {
        self.rendered_rows
    }
}

pub(crate) fn build_rendered_rows(lines: &[Line<'static>], width: usize) -> usize {
    HistoryCell::rendered_line_count(lines, width)
}

