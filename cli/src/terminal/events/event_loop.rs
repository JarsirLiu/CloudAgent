use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::Event as CEvent;
use tokio::sync::mpsc;

use super::broker::{EventBroker, EventLoopController};
use super::frame_requester::FrameRequester;
use super::input_source::{CrosstermInputSource, TerminalInputSource};
use super::types::UiEvent;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(120);

pub(crate) fn spawn_tui_event_loop() -> (
    mpsc::UnboundedReceiver<UiEvent>,
    FrameRequester,
    EventLoopController,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let tick_deadline = Arc::new((Mutex::new(None), Condvar::new()));
    let frame_requester = FrameRequester::new(tx.clone(), tick_deadline.clone());
    let broker = EventBroker::new();
    let controller = EventLoopController::new(broker.clone());

    spawn_scheduled_tick_loop(tx.clone(), tick_deadline);
    spawn_input_event_loop(tx, CrosstermInputSource, broker);

    (rx, frame_requester, controller)
}

fn spawn_input_event_loop(
    tx: mpsc::UnboundedSender<UiEvent>,
    mut source: impl TerminalInputSource + Send + 'static,
    broker: EventBroker,
) {
    std::thread::spawn(move || {
        loop {
            broker.wait_until_running();
            match source.poll(EVENT_POLL_INTERVAL) {
                Ok(true) => {
                    let Ok(event) = source.read() else {
                        break;
                    };
                    if let Some(event) = map_crossterm_event(event)
                        && tx.send(event).is_err()
                    {
                        break;
                    }
                }
                Ok(false) => {
                    if tx.send(UiEvent::Tick).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

pub(super) fn map_crossterm_event(event: CEvent) -> Option<UiEvent> {
    match event {
        CEvent::Key(key) => Some(UiEvent::Key(key)),
        CEvent::Paste(text) => Some(UiEvent::Paste(text)),
        CEvent::Mouse(mouse) => Some(UiEvent::Mouse(mouse)),
        CEvent::Resize(_, _) => Some(UiEvent::Resize),
        _ => None,
    }
}

fn spawn_scheduled_tick_loop(
    tx: mpsc::UnboundedSender<UiEvent>,
    tick_deadline: Arc<(Mutex<Option<Instant>>, Condvar)>,
) {
    std::thread::spawn(move || {
        let (deadline, wakeup) = &*tick_deadline;
        let mut guard = deadline.lock().expect("tick deadline poisoned");
        loop {
            let Some(next_deadline) = *guard else {
                guard = wakeup.wait(guard).expect("tick deadline poisoned");
                continue;
            };

            let now = Instant::now();
            if now >= next_deadline {
                *guard = None;
                drop(guard);
                if tx.send(UiEvent::Tick).is_err() {
                    break;
                }
                guard = deadline.lock().expect("tick deadline poisoned");
                continue;
            }

            let wait_for = next_deadline.saturating_duration_since(now);
            let (next_guard, _) = wakeup
                .wait_timeout(guard, wait_for)
                .expect("tick deadline poisoned");
            guard = next_guard;
        }
    });
}
