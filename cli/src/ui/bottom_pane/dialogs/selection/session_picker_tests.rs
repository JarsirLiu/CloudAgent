use super::{SessionPicker, SessionPickerMode};
use crate::ui::bottom_pane::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use agent_core::ConversationSummary;
use crossterm::event::{KeyCode, KeyEvent};

fn summary(id: &str, updated_at_ms: u64) -> ConversationSummary {
    ConversationSummary {
        conversation_id: id.to_string(),
        title: Some(id.to_string()),
        message_count: 1,
        updated_at_ms,
    }
}

#[test]
fn down_near_page_end_requests_more_sessions() {
    let sessions = vec![
        summary("session-5", 5),
        summary("session-4", 4),
        summary("session-3", 3),
        summary("session-2", 2),
        summary("session-1", 1),
    ];
    let mut picker = SessionPicker::new_page(
        sessions,
        "session-5",
        SessionPickerMode::Switch,
        true,
        Some("cursor-1".to_string()),
    );

    let mut action = BottomPaneViewAction::None;
    for _ in 0..2 {
        action = picker.handle_key_event(KeyEvent::from(KeyCode::Down));
    }

    assert!(matches!(
        action,
        BottomPaneViewAction::LoadMoreSessions { cursor } if cursor == "cursor-1"
    ));
    assert!(
        picker
            .render_lines(80)
            .iter()
            .any(|line| line.to_string().contains("Loading more sessions"))
    );
}

#[test]
fn append_page_deduplicates_and_clears_loading_state() {
    let mut picker = SessionPicker::new_page(
        vec![summary("session-2", 2), summary("session-1", 1)],
        "session-2",
        SessionPickerMode::Switch,
        true,
        Some("cursor-1".to_string()),
    );

    assert!(matches!(
        picker.handle_key_event(KeyEvent::from(KeyCode::Down)),
        BottomPaneViewAction::LoadMoreSessions { .. }
    ));

    assert!(picker.append_session_page(
        vec![summary("session-1", 1), summary("session-0", 0)],
        false,
        None,
    ));
    let rendered = picker
        .render_lines(80)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("session-0"));
    assert!(!rendered.contains("Loading more sessions"));
    assert!(!rendered.contains("more sessions below"));
}
