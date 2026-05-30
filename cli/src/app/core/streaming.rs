use crate::ui::widgets::history_cell::{HistoryCell, HistoryFormat};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentStreamOutput {
    pub(crate) stable_cells: Vec<HistoryCell>,
    pub(crate) live_cell: Option<HistoryCell>,
}

#[derive(Debug, Default)]
pub(crate) struct AgentStreamController {
    item_id: String,
    source: String,
    stable_source_len: usize,
    emitted_any: bool,
}

impl AgentStreamController {
    pub(crate) fn new(item_id: impl Into<String>) -> Self {
        Self {
            item_id: item_id.into(),
            ..Self::default()
        }
    }

    pub(crate) fn item_id(&self) -> &str {
        &self.item_id
    }

    pub(crate) fn push_delta(&mut self, delta: &str) -> AgentStreamOutput {
        self.source.push_str(delta);
        self.project()
    }

    pub(crate) fn finish(mut self) -> AgentStreamFinish {
        let mut stable_cells = Vec::new();
        if self.stable_source_len < self.source.len() {
            stable_cells.push(self.agent_cell(
                self.source[self.stable_source_len..].to_string(),
                self.emitted_any,
            ));
            self.stable_source_len = self.source.len();
            self.emitted_any = true;
        }
        AgentStreamFinish {
            emitted_any: self.emitted_any,
            stable_cells,
        }
    }

    fn project(&mut self) -> AgentStreamOutput {
        let stable_boundary = stable_source_boundary(&self.source);
        let target_stable_len = stable_boundary.max(self.stable_source_len);
        let mut stable_cells = Vec::new();
        if target_stable_len > self.stable_source_len {
            stable_cells.push(self.agent_cell(
                self.source[self.stable_source_len..target_stable_len].to_string(),
                self.emitted_any,
            ));
            self.stable_source_len = target_stable_len;
            self.emitted_any = true;
        }

        let live_source = self.source[self.stable_source_len..].to_string();
        AgentStreamOutput {
            stable_cells,
            live_cell: (!live_source.is_empty()).then(|| {
                HistoryCell::agent("", live_source, HistoryFormat::Markdown)
                    .with_stream_continuation(self.emitted_any)
            }),
        }
    }

    fn agent_cell(&self, source: String, continuation: bool) -> HistoryCell {
        HistoryCell::agent("", source, HistoryFormat::Markdown)
            .with_stream_continuation(continuation)
    }
}

pub(crate) struct AgentStreamFinish {
    pub(crate) emitted_any: bool,
    pub(crate) stable_cells: Vec<HistoryCell>,
}

fn stable_source_boundary(source: &str) -> usize {
    let complete_end = last_complete_line_end(source);
    if complete_end == 0 {
        return 0;
    }
    let complete = &source[..complete_end];
    let table_holdback = table_holdback_start(complete);
    let pending_header = pending_table_header_start(complete);
    table_holdback.or(pending_header).unwrap_or(complete_end)
}

fn last_complete_line_end(source: &str) -> usize {
    source.rfind('\n').map(|index| index + 1).unwrap_or(0)
}

fn table_holdback_start(source: &str) -> Option<usize> {
    let lines = line_ranges(source);
    for pair in lines.windows(2) {
        let header = &source[pair[0].0..pair[0].1];
        let delimiter = &source[pair[1].0..pair[1].1];
        if looks_like_table_header(header) && looks_like_table_delimiter(delimiter) {
            return Some(pair[0].0);
        }
    }
    None
}

fn pending_table_header_start(source: &str) -> Option<usize> {
    let lines = line_ranges(source);
    let &(start, end) = lines.last()?;
    let line = &source[start..end];
    looks_like_table_header(line).then_some(start)
}

fn line_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    for (index, ch) in source.char_indices() {
        if ch == '\n' {
            ranges.push((start, index));
            start = index + ch.len_utf8();
        }
    }
    if start < source.len() {
        ranges.push((start, source.len()));
    }
    ranges
}

fn looks_like_table_header(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|')
        && trimmed
            .split('|')
            .filter(|cell| !cell.trim().is_empty())
            .count()
            >= 2
}

fn looks_like_table_delimiter(line: &str) -> bool {
    let trimmed = line.trim().trim_matches('|').trim();
    if trimmed.is_empty() || !trimmed.contains('-') {
        return false;
    }
    trimmed.split('|').all(|cell| {
        let cell = cell.trim();
        !cell.is_empty() && cell.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bodies(cells: Vec<HistoryCell>) -> Vec<String> {
        cells
            .into_iter()
            .map(|cell| cell.body().to_string())
            .collect()
    }

    #[test]
    fn commits_complete_non_table_lines_and_keeps_tail_live() {
        let mut stream = AgentStreamController::new("a1");
        let output = stream.push_delta("stable line\nlive tail");

        assert_eq!(bodies(output.stable_cells), vec!["stable line\n"]);
        assert_eq!(
            output.live_cell.as_ref().map(|cell| cell.body()),
            Some("live tail")
        );
    }

    #[test]
    fn holds_pending_table_header_out_of_stable_history() {
        let mut stream = AgentStreamController::new("a1");
        let output = stream.push_delta("intro\n| a | b |\n");

        assert_eq!(bodies(output.stable_cells), vec!["intro\n"]);
        assert_eq!(
            output.live_cell.as_ref().map(|cell| cell.body()),
            Some("| a | b |\n")
        );
    }

    #[test]
    fn holds_confirmed_table_until_finalize() {
        let mut stream = AgentStreamController::new("a1");
        let first = stream.push_delta("intro\n| a | b |\n");
        let second = stream.push_delta("| - | - |\n| 1 | 2 |\n");

        assert_eq!(bodies(first.stable_cells), vec!["intro\n"]);
        assert!(second.stable_cells.is_empty());
        assert_eq!(
            second.live_cell.as_ref().map(|cell| cell.body()),
            Some("| a | b |\n| - | - |\n| 1 | 2 |\n")
        );

        let finish = stream.finish();
        assert_eq!(
            bodies(finish.stable_cells),
            vec!["| a | b |\n| - | - |\n| 1 | 2 |\n"]
        );
    }

    #[test]
    fn releases_pipe_line_when_next_line_is_not_table_delimiter() {
        let mut stream = AgentStreamController::new("a1");
        let first = stream.push_delta("a | b\n");
        let second = stream.push_delta("plain next\n");

        assert!(first.stable_cells.is_empty());
        assert_eq!(bodies(second.stable_cells), vec!["a | b\nplain next\n"]);
        assert!(second.live_cell.is_none());
    }
}
