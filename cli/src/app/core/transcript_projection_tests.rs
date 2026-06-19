use crate::app::core::transcript_owner::TranscriptOwner;
use agent_core::conversation::InputItem;

fn local_input(text: &str) -> Vec<InputItem> {
    vec![InputItem::Text {
        text: text.to_string(),
    }]
}

#[test]
fn viewport_snapshot_keeps_only_live_cells() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.push_live_cell(crate::ui::history_cell::HistoryCell::info(
        "conversation",
        "temporary notice",
        crate::ui::history_cell::HistoryTone::Warning,
    ));

    let snapshot = owner.viewport_snapshot();
    let rendered = snapshot
        .cells
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();

    assert_eq!(rendered, vec!["temporary notice".to_string()]);
}

#[test]
fn scrollback_snapshot_keeps_only_committed_cells() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("first"), false);
    owner.push_live_cell(crate::ui::history_cell::HistoryCell::reasoning(
        "Reasoning",
        "thinking",
    ));

    let scrollback = owner.scrollback_snapshot();
    let scrollback_bodies = scrollback
        .cells
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();

    assert_eq!(scrollback_bodies, vec!["first".to_string()]);
}

#[test]
fn snapshot_revisions_track_committed_and_live_updates_independently() {
    let mut owner = TranscriptOwner::default();
    let initial_scrollback = owner.scrollback_snapshot().revision;
    let initial_viewport = owner.viewport_snapshot().revision;

    owner.push_committed_cell(crate::ui::history_cell::HistoryCell::user("hello"));
    let after_committed_scrollback = owner.scrollback_snapshot().revision;
    let after_committed_viewport = owner.viewport_snapshot().revision;
    assert_ne!(after_committed_scrollback, initial_scrollback);
    assert_eq!(after_committed_viewport, initial_viewport);

    owner.push_live_cell(crate::ui::history_cell::HistoryCell::info(
        "conversation",
        "notice",
        crate::ui::history_cell::HistoryTone::Control,
    ));
    let after_live_scrollback = owner.scrollback_snapshot().revision;
    let after_live_viewport = owner.viewport_snapshot().revision;
    assert_eq!(after_live_scrollback, after_committed_scrollback);
    assert_ne!(after_live_viewport, after_committed_viewport);
}
