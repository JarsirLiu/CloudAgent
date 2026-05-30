use crossterm::event::{self, Event as CEvent, KeyEvent};
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
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
    tick_deadline: Arc<(Mutex<Option<Instant>>, Condvar)>,
}

impl FrameRequester {
    pub(crate) fn schedule_frame(&self) {
        if !self.draw_pending.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(UiEvent::Draw);
        }
    }

    pub(crate) fn schedule_tick_in(&self, delay: Duration) {
        let (deadline, wakeup) = &*self.tick_deadline;
        let mut deadline = deadline.lock().expect("tick deadline poisoned");
        *deadline = Some(Instant::now() + delay);
        wakeup.notify_one();
    }

    pub(crate) fn finish_draw(&self) {
        self.draw_pending.store(false, Ordering::Release);
    }
}

pub(crate) fn spawn_tui_event_loop() -> (mpsc::UnboundedReceiver<UiEvent>, FrameRequester) {
    let (tx, rx) = mpsc::unbounded_channel();
    let tick_deadline = Arc::new((Mutex::new(None), Condvar::new()));
    let frame_requester = FrameRequester {
        tx: tx.clone(),
        draw_pending: Arc::new(AtomicBool::new(false)),
        tick_deadline: tick_deadline.clone(),
    };
    spawn_scheduled_tick_loop(tx.clone(), tick_deadline);
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
