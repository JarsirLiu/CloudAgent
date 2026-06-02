use agent_core::InputItem;
use agent_protocol::FrontendMode;

#[derive(Clone, Debug)]
pub(crate) struct TurnLifecycleState {
    pending_submitted_input: Option<Vec<InputItem>>,
    interrupt_requested: bool,
    frontend_mode: FrontendMode,
}

impl TurnLifecycleState {
    pub(crate) fn new() -> Self {
        Self {
            pending_submitted_input: None,
            interrupt_requested: false,
            frontend_mode: FrontendMode::Idle,
        }
    }

    pub(crate) fn begin_submit(&mut self, content: &[InputItem]) {
        self.pending_submitted_input = Some(content.to_vec());
        self.interrupt_requested = false;
        self.frontend_mode = FrontendMode::Running;
    }

    pub(crate) fn request_interrupt(&mut self) {
        self.interrupt_requested = true;
    }

    pub(crate) fn sync_frontend_mode(&mut self, mode: FrontendMode) {
        if mode == FrontendMode::Idle {
            self.interrupt_requested = false;
        }
        self.frontend_mode = mode;
    }

    pub(crate) fn finish_turn(&mut self) {
        self.interrupt_requested = false;
        self.frontend_mode = FrontendMode::Idle;
    }

    pub(crate) fn clear_pending_submission(&mut self) {
        self.pending_submitted_input = None;
    }

    pub(crate) fn take_pending_submission(&mut self) -> Option<Vec<InputItem>> {
        self.pending_submitted_input.take()
    }

    pub(crate) fn recover_orphaned(&mut self) {
        self.interrupt_requested = false;
        self.pending_submitted_input = None;
        self.frontend_mode = FrontendMode::Idle;
    }

    #[cfg(test)]
    pub(crate) fn pending_submission(&self) -> Option<&[InputItem]> {
        self.pending_submitted_input.as_deref()
    }

    pub(crate) fn interrupt_requested(&self) -> bool {
        self.interrupt_requested
    }

    pub(crate) fn frontend_mode(&self) -> FrontendMode {
        self.frontend_mode
    }
}

impl Default for TurnLifecycleState {
    fn default() -> Self {
        Self::new()
    }
}
