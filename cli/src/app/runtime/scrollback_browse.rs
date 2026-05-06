use std::time::{Duration, Instant};

const SCROLLBACK_BROWSE_GRACE: Duration = Duration::from_millis(900);

#[derive(Default)]
pub(crate) struct ScrollbackBrowseState {
    active_until: Option<Instant>,
}

impl ScrollbackBrowseState {
    pub(crate) fn enter(&mut self) {
        self.active_until = Some(Instant::now() + SCROLLBACK_BROWSE_GRACE);
    }

    pub(crate) fn leave(&mut self) {
        self.active_until = None;
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active_until
            .is_some_and(|until| Instant::now() < until)
    }
}

#[cfg(test)]
mod tests {
    use super::ScrollbackBrowseState;

    #[test]
    fn enter_marks_scrollback_browsing_active() {
        let mut state = ScrollbackBrowseState::default();

        assert!(!state.is_active());
        state.enter();
        assert!(state.is_active());
    }

    #[test]
    fn leave_clears_scrollback_browsing_state() {
        let mut state = ScrollbackBrowseState::default();

        state.enter();
        assert!(state.is_active());

        state.leave();
        assert!(!state.is_active());
    }
}
