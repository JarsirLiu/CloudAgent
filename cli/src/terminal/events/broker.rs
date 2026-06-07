use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Debug)]
pub struct EventLoopController {
    broker: EventBroker,
}

impl EventLoopController {
    pub fn pause_events(&self) {
        self.broker.pause();
    }

    pub fn resume_events(&self) {
        self.broker.resume();
    }

    pub(super) fn new(broker: EventBroker) -> Self {
        Self { broker }
    }
}

#[derive(Clone, Debug)]
pub(super) struct EventBroker {
    state: Arc<(Mutex<EventBrokerState>, Condvar)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventBrokerState {
    Running,
    Paused,
}

impl EventBroker {
    pub(super) fn new() -> Self {
        Self {
            state: Arc::new((Mutex::new(EventBrokerState::Running), Condvar::new())),
        }
    }

    pub(super) fn wait_until_running(&self) {
        let (state, wakeup) = &*self.state;
        let mut state = state.lock().expect("event broker state poisoned");
        while *state == EventBrokerState::Paused {
            state = wakeup.wait(state).expect("event broker state poisoned");
        }
    }

    pub(super) fn pause(&self) {
        let (state, _) = &*self.state;
        *state.lock().expect("event broker state poisoned") = EventBrokerState::Paused;
    }

    pub(super) fn resume(&self) {
        let (state, wakeup) = &*self.state;
        *state.lock().expect("event broker state poisoned") = EventBrokerState::Running;
        wakeup.notify_all();
    }
}
