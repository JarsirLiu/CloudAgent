use crossterm::event::{self, Event as CEvent, KeyEvent};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

pub(crate) enum UiEvent {
    Key(KeyEvent),
    Paste(String),
    Resize,
    Tick,
    Draw,
}

#[derive(Clone)]
pub(crate) struct FrameRequester {
    tx: mpsc::UnboundedSender<UiEvent>,
    draw_pending: Arc<AtomicBool>,
}

impl FrameRequester {
    pub(crate) fn schedule_frame(&self) {
        if !self.draw_pending.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(UiEvent::Draw);
        }
    }

    pub(crate) fn finish_draw(&self) {
        self.draw_pending.store(false, Ordering::Release);
    }
}

pub(crate) fn spawn_tui_event_loop() -> (mpsc::UnboundedReceiver<UiEvent>, FrameRequester) {
    let (tx, rx) = mpsc::unbounded_channel();
    let frame_requester = FrameRequester {
        tx: tx.clone(),
        draw_pending: Arc::new(AtomicBool::new(false)),
    };
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
                    Ok(CEvent::Mouse(_)) => {}
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
    (rx, frame_requester)
}
