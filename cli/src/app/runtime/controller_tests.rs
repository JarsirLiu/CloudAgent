use super::should_request_older_history_page;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn page_up_and_home_request_older_history() {
    let page_up = KeyEvent::new(KeyCode::PageUp, KeyModifiers::empty());
    let home = KeyEvent::new(KeyCode::Home, KeyModifiers::empty());
    let other = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty());

    assert!(should_request_older_history_page(page_up));
    assert!(should_request_older_history_page(home));
    assert!(!should_request_older_history_page(other));
}
