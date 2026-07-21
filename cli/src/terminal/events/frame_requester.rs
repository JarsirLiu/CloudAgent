use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use super::types::UiEvent;

#[derive(Clone)]
pub(crate) struct FrameRequester {
    pub(super) tx: mpsc::UnboundedSender<UiEvent>,
    pub(super) draw_pending: Arc<AtomicBool>,
    pub(super) tick_deadline: Arc<(Mutex<Option<Instant>>, Condvar)>,
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

    pub(super) fn new(
        tx: mpsc::UnboundedSender<UiEvent>,
        tick_deadline: Arc<(Mutex<Option<Instant>>, Condvar)>,
    ) -> Self {
        Self {
            tx,
            draw_pending: Arc::new(AtomicBool::new(false)),
            tick_deadline,
        }
    }
}
