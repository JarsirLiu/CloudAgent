use crate::ui::history_cell::{HistoryCell, HistoryFormat};

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

    pub(crate) fn current_live_cell(&self) -> Option<HistoryCell> {
        let live_source = self.source[self.stable_source_len..].to_string();
        (!live_source.is_empty()).then(|| self.agent_cell(live_source, self.emitted_any))
    }

    pub(crate) fn finish_with_final_source(
        mut self,
        final_source: Option<&str>,
    ) -> AgentStreamFinish {
        let mut stable_cells = Vec::new();
        let source = match final_source.filter(|text| !text.is_empty()) {
            Some(text) if text.starts_with(&self.source) => text,
            _ => &self.source,
        };

        if self.stable_source_len < source.len() {
            stable_cells.push(self.agent_cell(
                source[self.stable_source_len..].to_string(),
                self.emitted_any,
            ));
            self.stable_source_len = source.len();
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
                    .with_provisional_stream(true)
                    .with_stream_item_id(self.item_id.clone())
            }),
        }
    }

    fn agent_cell(&self, source: String, continuation: bool) -> HistoryCell {
        HistoryCell::agent("", source, HistoryFormat::Markdown)
            .with_stream_continuation(continuation)
            .with_provisional_stream(true)
            .with_stream_item_id(self.item_id.clone())
    }
}

pub(crate) struct AgentStreamFinish {
    pub(crate) emitted_any: bool,
    pub(crate) stable_cells: Vec<HistoryCell>,
}

fn stable_source_boundary(source: &str) -> usize {
    let mut boundary = 0usize;
    let mut in_fenced_code_block = false;

    for (start, end) in complete_line_ranges(source) {
        let line = &source[start..end];
        let trimmed = line.trim_end_matches('\n');
        if is_fenced_code_line(trimmed) {
            in_fenced_code_block = !in_fenced_code_block;
            continue;
        }
        if !in_fenced_code_block && trimmed.trim().is_empty() {
            boundary = end;
        }
    }

    boundary
}

fn complete_line_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    for (index, ch) in source.char_indices() {
        if ch == '\n' {
            ranges.push((start, index + ch.len_utf8()));
            start = index + ch.len_utf8();
        }
    }
    ranges
}

fn is_fenced_code_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
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
        let output = stream.push_delta("stable line\n\nlive tail");

        assert_eq!(bodies(output.stable_cells), vec!["stable line\n\n"]);
        assert_eq!(
            output.live_cell.as_ref().map(|cell| cell.body()),
            Some("live tail")
        );
    }

    #[test]
    fn keeps_incomplete_markdown_block_live_without_blank_line() {
        let mut stream = AgentStreamController::new("a1");
        let output = stream.push_delta("intro\n| a | b |\n");

        assert!(output.stable_cells.is_empty());
        assert_eq!(
            output.live_cell.as_ref().map(|cell| cell.body()),
            Some("intro\n| a | b |\n")
        );
    }

    #[test]
    fn commits_previous_block_before_next_blank_line() {
        let mut stream = AgentStreamController::new("a1");
        let first = stream.push_delta("intro\n\n| a | b |\n");
        let second = stream.push_delta("| - | - |\n| 1 | 2 |\n");

        assert_eq!(bodies(first.stable_cells), vec!["intro\n\n"]);
        assert!(second.stable_cells.is_empty());
        assert_eq!(
            second.live_cell.as_ref().map(|cell| cell.body()),
            Some("| a | b |\n| - | - |\n| 1 | 2 |\n")
        );

        let finish = stream.finish_with_final_source(None);
        assert_eq!(
            bodies(finish.stable_cells),
            vec!["| a | b |\n| - | - |\n| 1 | 2 |\n"]
        );
    }

    #[test]
    fn keeps_fenced_code_block_live_until_completion() {
        let mut stream = AgentStreamController::new("a1");
        let first = stream.push_delta("intro\n\n```rust\nfn main() {\n");
        let second = stream.push_delta("println!(\"hi\");\n}\n```\n");

        assert_eq!(bodies(first.stable_cells), vec!["intro\n\n"]);
        assert!(second.stable_cells.is_empty());
        assert_eq!(
            second.live_cell.as_ref().map(|cell| cell.body()),
            Some("```rust\nfn main() {\nprintln!(\"hi\");\n}\n```\n")
        );

        let finish = stream.finish_with_final_source(None);
        assert_eq!(
            bodies(finish.stable_cells),
            vec!["```rust\nfn main() {\nprintln!(\"hi\");\n}\n```\n"]
        );
    }
}

