use super::viewport_height::ViewportHeightPolicy;
use crate::app::TuiApp;
use crate::app::core::conversation_state::conversation_view_snapshot_for_test;
use crate::ui::chat_surface::ChatSurface;
use crate::ui::history_cell::{HistoryCell, HistoryFormat};
use ratatui::layout::Rect;

#[test]
fn running_live_stream_keeps_viewport_height_stable_for_same_terminal_area() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.apply_conversation_view_snapshot(conversation_view_snapshot_for_test(
        &app.conversation_id,
        agent_protocol::FrontendMode::Running,
    ));
    app.push_live_cell(HistoryCell::agent(
        "assistant",
        "short body",
        HistoryFormat::Markdown,
    ));

    let mut policy = ViewportHeightPolicy::default();
    let area = Rect::new(0, 0, 120, 40);
    let initial_height = policy.resolve(&mut app, area);

    app.push_live_cell(HistoryCell::agent(
        "assistant",
        "This is a much longer live body that should normally change the viewport height if the layout were allowed to follow the stream content on every frame. "
            .repeat(4),
        HistoryFormat::Markdown,
    ));

    let updated_height = policy.resolve(&mut app, area);

    assert_eq!(updated_height, initial_height);
}

#[test]
fn viewport_height_lock_is_released_when_terminal_area_changes() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.apply_conversation_view_snapshot(conversation_view_snapshot_for_test(
        &app.conversation_id,
        agent_protocol::FrontendMode::Running,
    ));
    app.push_live_cell(HistoryCell::agent(
        "assistant",
        "This live body is intentionally long enough to wrap differently when the terminal width changes, so the cached height should be refreshed instead of reused blindly.",
        HistoryFormat::Markdown,
    ));

    let mut policy = ViewportHeightPolicy::default();
    let first_area = Rect::new(0, 0, 120, 40);
    let second_area = Rect::new(0, 0, 72, 40);

    let initial_height = policy.resolve(&mut app, first_area);
    let resized_height = policy.resolve(&mut app, second_area);
    let expected_second_height = ChatSurface::desired_viewport_height(&mut app, second_area);

    assert_eq!(resized_height, expected_second_height);
    assert_ne!(resized_height, initial_height);
}
