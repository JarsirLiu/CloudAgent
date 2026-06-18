use super::paste_coordinator::PasteCoordinator;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn text_shortcut_is_only_enabled_for_text_paste_views() {
    let coordinator = PasteCoordinator::default();
    let key = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);

    assert!(coordinator.should_handle_text_shortcut(key, true));
    assert!(!coordinator.should_handle_text_shortcut(key, false));
}

#[test]
fn matching_terminal_paste_is_suppressed_after_shortcut() {
    let mut coordinator = PasteCoordinator::default();
    coordinator.record_shortcut_text("token");

    assert!(!coordinator.decide_terminal_paste("token").should_forward());
    assert!(coordinator.decide_terminal_paste("token").should_forward());
}
