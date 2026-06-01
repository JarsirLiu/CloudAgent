use agent_core::rollout::RolloutItem;
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) struct ParsedRolloutLine {
    pub item: RolloutItem,
    pub start_offset: u64,
}

pub(crate) fn parse_items(path: &Path, text: &str) -> Result<Vec<RolloutItem>> {
    parse_lines_with_offsets(path, text)
        .map(|lines| lines.into_iter().map(|line| line.item).collect())
}

pub(crate) fn parse_lines_with_offsets(path: &Path, text: &str) -> Result<Vec<ParsedRolloutLine>> {
    let mut lines = Vec::new();
    let mut offset = 0u64;
    let mut line_no = 0usize;
    let final_line_may_be_truncated = !text.is_empty() && !text.ends_with('\n');

    for segment in text.split_inclusive('\n') {
        line_no += 1;
        let line_start_offset = offset;
        offset = offset.saturating_add(segment.len() as u64);
        let is_final_unterminated = final_line_may_be_truncated && offset as usize == text.len();
        if let Some(item) = parse_line(path, line_no, segment, is_final_unterminated)? {
            lines.push(ParsedRolloutLine {
                item,
                start_offset: line_start_offset,
            });
        }
    }

    if final_line_may_be_truncated && (offset as usize) < text.len() {
        line_no += 1;
        let segment = &text[offset as usize..];
        if let Some(item) = parse_line(path, line_no, segment, true)? {
            lines.push(ParsedRolloutLine {
                item,
                start_offset: offset,
            });
        }
    }

    Ok(lines)
}

fn parse_line(
    path: &Path,
    line_no: usize,
    line: &str,
    is_final_unterminated: bool,
) -> Result<Option<RolloutItem>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    match serde_json::from_str::<RolloutItem>(line) {
        Ok(item) => Ok(Some(item)),
        Err(err) if is_final_unterminated => {
            tracing::warn!(
                path = %path.display(),
                line = line_no,
                error = %err,
                "ignoring truncated rollout tail"
            );
            Ok(None)
        }
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to parse rollout file {} at line {}",
                path.display(),
                line_no
            )
        }),
    }
}
