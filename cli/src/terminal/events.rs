use crossterm::event::{self, Event as CEvent, KeyEvent, MouseEvent};
use std::time::Duration;
use tokio::sync::mpsc;

pub(crate) enum UiEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize,
    Tick,
}

pub(crate) fn spawn_tui_event_loop() -> mpsc::UnboundedReceiver<UiEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        loop {
            match event::poll(Duration::from_millis(120)) {
                Ok(true) => match event::read() {
                    Ok(CEvent::Key(key)) => {
                        if tx.send(UiEvent::Key(key)).is_err() {
                            break;
                        }
                    }
                    Ok(CEvent::Paste(text)) => {
                        if tx.send(UiEvent::Paste(text)).is_err() {
                            break;
                        }
                    }
                    Ok(CEvent::Mouse(mouse)) => {
                        if tx.send(UiEvent::Mouse(mouse)).is_err() {
                            break;
                        }
                    }
                    Ok(CEvent::Resize(_, _)) => {
                        if tx.send(UiEvent::Resize).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                },
                Ok(false) => {
                    if tx.send(UiEvent::Tick).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}
