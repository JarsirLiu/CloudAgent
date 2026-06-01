use ratatui::text::Line;

use crate::terminal::{HistoryRenderMetrics, prepare_history_lines, prepare_history_tail_lines};
use crate::ui::widgets::history_cell::HistoryCell;

#[derive(Default)]
pub(crate) struct HistoryFlushQueue {
    pending_lines: Vec<Line<'static>>,
}

impl HistoryFlushQueue {
    pub(crate) fn replace_with_replay(
        &mut self,
        cells: Vec<HistoryCell>,
        render_metrics: HistoryRenderMetrics,
        max_rows: Option<usize>,
    ) {
        self.pending_lines = if let Some(max_rows) = max_rows {
            prepare_history_tail_lines(cells, render_metrics, max_rows)
        } else {
            prepare_history_lines(cells, render_metrics, false)
        };
    }

    pub(crate) fn append_tail(
        &mut self,
        cells: Vec<HistoryCell>,
        render_metrics: HistoryRenderMetrics,
        has_visible_history: bool,
    ) {
        let has_history = has_visible_history || !self.pending_lines.is_empty();
        self.pending_lines
            .extend(prepare_history_lines(cells, render_metrics, has_history));
    }

    pub(crate) fn pending_lines(&self) -> Vec<Line<'static>> {
        self.pending_lines.clone()
    }

    pub(crate) fn mark_flushed(&mut self) {
        self.pending_lines.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::HistoryFlushQueue;
    use crate::terminal::HistoryRenderMetrics;
    use crate::ui::widgets::history_cell::HistoryCell;

    fn metrics(width: usize) -> HistoryRenderMetrics {
        HistoryRenderMetrics {
            width,
            left_padding: 0,
        }
    }

    fn plain(lines: Vec<ratatui::text::Line<'static>>) -> Vec<String> {
        lines
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn append_tail_keeps_pending_lines_until_marked_flushed() {
        let mut queue = HistoryFlushQueue::default();

        queue.append_tail(vec![HistoryCell::user("first")], metrics(80), false);
        queue.append_tail(vec![HistoryCell::user("second")], metrics(80), false);

        let lines = plain(queue.pending_lines());
        assert_eq!(lines, vec!["› first", "", "› second"]);

        queue.mark_flushed();
        assert!(queue.pending_lines().is_empty());
    }

    #[test]
    fn replay_replaces_pending_append_lines() {
        let mut queue = HistoryFlushQueue::default();

        queue.append_tail(vec![HistoryCell::user("stale")], metrics(80), false);
        queue.replace_with_replay(vec![HistoryCell::user("canonical")], metrics(80), None);

        let lines = plain(queue.pending_lines());
        assert_eq!(lines, vec!["› canonical"]);
    }

    #[test]
    fn replay_can_be_capped_to_latest_rows() {
        let mut queue = HistoryFlushQueue::default();

        queue.replace_with_replay(
            vec![
                HistoryCell::user("oldest message"),
                HistoryCell::user("middle message"),
                HistoryCell::user("latest message"),
            ],
            metrics(80),
            Some(3),
        );

        let lines = plain(queue.pending_lines());
        let rendered = lines.join("\n");
        assert!(lines.len() <= 3);
        assert!(rendered.contains("latest message"));
        assert!(!rendered.contains("oldest message"));
    }
}
