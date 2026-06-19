use super::{HistoryCellGapKey, TranscriptLineOptions, build_transcript_lines};
use crate::ui::widgets::history_cell::{HistoryCell, HistoryFormat, HistoryKind, HistoryTone};

#[test]
fn live_transcript_keeps_message_and_agent_lines() {
    let cells = vec![
        HistoryCell::user("hello"),
        HistoryCell::agent("assistant", "world", HistoryFormat::Markdown),
    ];

    let build = build_transcript_lines(&cells, TranscriptLineOptions::live(80));
    let rendered = build
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    assert!(rendered.iter().any(|line| line.contains("hello")));
    assert!(rendered.iter().any(|line| line.contains("world")));
    assert_eq!(
        build.last_cell.map(|cell| cell.kind),
        Some(HistoryKind::Message)
    );
}

#[test]
fn scrollback_inserts_gap_for_distinct_cells() {
    let cells = vec![
        HistoryCell::user("hello"),
        HistoryCell::agent("assistant", "world", HistoryFormat::Markdown),
    ];

    let build = build_transcript_lines(&cells, TranscriptLineOptions::scrollback(80, None));
    let rendered = build
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    assert!(rendered.iter().any(|line| line.contains("hello")));
    assert!(rendered.iter().any(|line| line.contains("world")));
    assert!(rendered.iter().any(|line| line.is_empty()));
}

#[test]
fn incremental_scrollback_inserts_gap_after_previous_context() {
    let previous = HistoryCellGapKey::from_cell(&HistoryCell::agent(
        "assistant",
        "answer",
        HistoryFormat::Markdown,
    ));
    let cells = vec![HistoryCell::user("next")];

    let build = build_transcript_lines(
        &cells,
        TranscriptLineOptions::scrollback(80, Some(previous)),
    );
    let rendered = build
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    assert_eq!(rendered.first().map(String::as_str), Some(""));
    assert!(rendered.iter().any(|line| line.contains("next")));
}

#[test]
fn continuation_cells_do_not_insert_gap() {
    let first = HistoryCell::agent("assistant", "hello", HistoryFormat::Markdown);
    let second = HistoryCell::agent("assistant", " world", HistoryFormat::Markdown)
        .with_stream_continuation(true);
    let cells = vec![first, second];

    let build = build_transcript_lines(&cells, TranscriptLineOptions::scrollback(80, None));
    let rendered = build
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    assert!(!rendered.iter().any(|line| line.is_empty()));
}

#[test]
fn live_mode_keeps_extra_gap_before_tool_like_cells() {
    let cells = vec![
        HistoryCell::agent("assistant", "answer", HistoryFormat::Markdown),
        HistoryCell::exec(
            "exec",
            "cargo test -p cli --lib",
            None,
            HistoryTone::Control,
        ),
    ];

    let build = build_transcript_lines(&cells, TranscriptLineOptions::live(80));
    let rendered = build
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    let first_command_line = rendered
        .iter()
        .position(|line| line.contains("cargo test"))
        .expect("command line should render");

    assert_eq!(
        rendered[..first_command_line]
            .iter()
            .filter(|line| line.is_empty())
            .count(),
        2
    );
}

#[test]
fn scrollback_mode_uses_single_gap_before_tool_like_cells() {
    let cells = vec![
        HistoryCell::agent("assistant", "answer", HistoryFormat::Markdown),
        HistoryCell::exec(
            "exec",
            "cargo test -p cli --lib",
            None,
            HistoryTone::Control,
        ),
    ];

    let build = build_transcript_lines(&cells, TranscriptLineOptions::scrollback(80, None));
    let rendered = build
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    let first_command_line = rendered
        .iter()
        .position(|line| line.contains("cargo test"))
        .expect("command line should render");

    assert_eq!(
        rendered[..first_command_line]
            .iter()
            .filter(|line| line.is_empty())
            .count(),
        1
    );
}