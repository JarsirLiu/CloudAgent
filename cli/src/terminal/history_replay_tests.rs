use super::{HistoryReplayBatch, HistoryReplayMode};
use ratatui::text::Line;

#[test]
fn append_batch_keeps_mode_lines_and_padding() {
    let batch = HistoryReplayBatch::append(vec![Line::from("hello")], 4);

    assert_eq!(batch.mode, HistoryReplayMode::Append);
    assert_eq!(batch.left_padding, 4);
    assert_eq!(batch.lines.len(), 1);
    assert_eq!(batch.lines[0].to_string(), "hello");
}

#[test]
fn full_replay_batch_keeps_mode_lines_and_padding() {
    let batch = HistoryReplayBatch::full_replay(vec![Line::from("hello")], 6);

    assert_eq!(batch.mode, HistoryReplayMode::FullReplay);
    assert_eq!(batch.left_padding, 6);
    assert_eq!(batch.lines.len(), 1);
    assert_eq!(batch.lines[0].to_string(), "hello");
}
