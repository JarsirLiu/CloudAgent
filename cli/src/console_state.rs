use agent_protocol::{AppServerMessage, AppServerNotification, AppServerRequest, FrontendMode, RequestId};

#[derive(Clone, Debug)]
pub(crate) struct ConsoleState {
    pub(crate) mode: FrontendMode,
    pub(crate) pending_approval_request_id: Option<RequestId>,
}

impl ConsoleState {
    pub(crate) fn new() -> Self {
        Self {
            mode: FrontendMode::Idle,
            pending_approval_request_id: None,
        }
    }

    pub(crate) fn can_submit_turn(&self) -> bool {
        self.mode == FrontendMode::Idle
    }

    pub(crate) fn update_from_message(&mut self, message: &AppServerMessage) {
        match message {
            AppServerMessage::Notification(notification) => match notification {
                AppServerNotification::FrontendStateChanged { mode, .. } => self.mode = *mode,
                AppServerNotification::TurnCompleted { .. }
                | AppServerNotification::TurnFailed { .. }
                | AppServerNotification::TurnCancelled { .. } => {
                    self.mode = FrontendMode::Idle;
                    self.pending_approval_request_id = None;
                }
                _ => {}
            },
            AppServerMessage::Request(AppServerRequest::Approval { request_id, .. }) => {
                self.mode = FrontendMode::WaitingForApproval;
                self.pending_approval_request_id = Some(request_id.clone());
            }
        }
    }
}

