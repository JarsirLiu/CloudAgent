use super::{ScrollbackDiff, TerminalProjectionController, scrollback_diff};
use crate::ui::history_cell::{HistoryCell, HistoryFormat};

#[test]
fn empty_committed_cells_do_not_emit_scrollback_update() {
    let mut app = crate::app::TuiApp::new(
        "default".to_string(),
        "test",
        std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    let mut projection = TerminalProjectionController::default();

    let update = projection.prepare_history_update(
        &mut app,
        crate::ui::chat_surface::TranscriptRenderMetrics {
            width: 80,
            left_padding: 4,
        },
        12,
    );

    assert!(update.is_none());
}

#[test]
fn viewport_height_change_does_not_replay_unchanged_scrollback() {
    let mut app = crate::app::TuiApp::new(
        "default".to_string(),
        "test",
        std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    let mut projection = TerminalProjectionController::default();
    app.transcript_owner
        .push_committed_cell(HistoryCell::user("hello"));

    let first = projection.prepare_history_update(
        &mut app,
        crate::ui::chat_surface::TranscriptRenderMetrics {
            width: 80,
            left_padding: 4,
        },
        10,
    );
    assert!(matches!(
        first.map(|batch| batch.mode),
        Some(crate::terminal::HistoryReplayMode::FullReplay)
    ));

    let second = projection.prepare_history_update(
        &mut app,
        crate::ui::chat_surface::TranscriptRenderMetrics {
            width: 80,
            left_padding: 4,
        },
        14,
    );
    assert!(second.is_none());
}

#[test]
fn render_metrics_change_still_forces_full_replay() {
    let mut app = crate::app::TuiApp::new(
        "default".to_string(),
        "test",
        std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    let mut projection = TerminalProjectionController::default();
    app.transcript_owner
        .push_committed_cell(HistoryCell::user("hello"));

    let first = projection.prepare_history_update(
        &mut app,
        crate::ui::chat_surface::TranscriptRenderMetrics {
            width: 80,
            left_padding: 4,
        },
        10,
    );
    assert!(first.is_some());

    let second = projection.prepare_history_update(
        &mut app,
        crate::ui::chat_surface::TranscriptRenderMetrics {
            width: 60,
            left_padding: 4,
        },
        10,
    );
    assert!(matches!(
        second.map(|batch| batch.mode),
        Some(crate::terminal::HistoryReplayMode::FullReplay)
    ));
}

#[test]
fn scrollback_diff_allows_append_only_updates() {
    let previous = vec![HistoryCell::user("hello")];
    let current = vec![
        HistoryCell::user("hello"),
        HistoryCell::agent("assistant", "world", HistoryFormat::Markdown),
    ];

    assert_eq!(
        scrollback_diff(&previous, &current),
        ScrollbackDiff::AppendFrom(1)
    );
}

#[test]
fn scrollback_diff_replays_when_existing_prefix_changes() {
    let previous = vec![
        HistoryCell::user("hello"),
        HistoryCell::agent("assistant", "old", HistoryFormat::Markdown),
    ];
    let current = vec![
        HistoryCell::user("hello"),
        HistoryCell::agent("assistant", "new", HistoryFormat::Markdown),
        HistoryCell::user("next"),
    ];

    assert_eq!(scrollback_diff(&previous, &current), ScrollbackDiff::Replay);
}
