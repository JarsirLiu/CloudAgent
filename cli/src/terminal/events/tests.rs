use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{Event as CEvent, KeyCode, KeyEvent, KeyModifiers};

use super::broker::EventBroker;
use super::event_loop::map_crossterm_event;
use super::types::UiEvent;

#[test]
fn maps_keyboard_events() {
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

    let Some(UiEvent::Key(mapped)) = map_crossterm_event(CEvent::Key(key)) else {
        panic!("expected key event");
    };

    assert_eq!(mapped, key);
}

#[test]
fn maps_paste_events() {
    let Some(UiEvent::Paste(mapped)) = map_crossterm_event(CEvent::Paste("hello".to_string()))
    else {
        panic!("expected paste event");
    };

    assert_eq!(mapped, "hello");
}

#[test]
fn maps_resize_events_without_leaking_dimensions() {
    assert!(matches!(
        map_crossterm_event(CEvent::Resize(120, 40)),
        Some(UiEvent::Resize)
    ));
}

#[test]
fn ignores_focus_events() {
    assert!(map_crossterm_event(CEvent::FocusGained).is_none());
    assert!(map_crossterm_event(CEvent::FocusLost).is_none());
}

#[test]
fn event_broker_blocks_until_resumed() {
    let broker = EventBroker::new();
    broker.pause();

    let waiting_broker = broker.clone();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        waiting_broker.wait_until_running();
        tx.send(()).expect("test receiver should be open");
    });

    assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());

    broker.resume();
    rx.recv_timeout(Duration::from_secs(1))
        .expect("paused event reader should resume");
}
