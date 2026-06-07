use agent_core::InputItem;

#[derive(Clone, Debug)]
pub(crate) struct TurnLifecycleState {
    pending_submitted_input: Option<Vec<InputItem>>,
}

impl TurnLifecycleState {
    pub(crate) fn new() -> Self {
        Self {
            pending_submitted_input: None,
        }
    }

    pub(crate) fn begin_submit(&mut self, content: &[InputItem]) {
        self.pending_submitted_input = Some(content.to_vec());
    }

    pub(crate) fn finish_turn(&mut self) {}

    pub(crate) fn clear_pending_submission(&mut self) {
        self.pending_submitted_input = None;
    }

    pub(crate) fn take_pending_submission(&mut self) -> Option<Vec<InputItem>> {
        self.pending_submitted_input.take()
    }

    #[cfg(test)]
    pub(crate) fn pending_submission(&self) -> Option<&[InputItem]> {
        self.pending_submitted_input.as_deref()
    }
}

impl Default for TurnLifecycleState {
    fn default() -> Self {
        Self::new()
    }
}
