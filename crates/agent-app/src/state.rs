use agent_protocol::{ApprovalDecision, ApprovalRequest, AppServerEvent, FrontendMode};
use tokio::sync::oneshot;

pub(crate) struct PendingApproval {
    pub(crate) reply: oneshot::Sender<ApprovalDecision>,
}

pub(crate) struct ConsoleState {
    mode: FrontendMode,
    pending_approval: Option<PendingApproval>,
}

impl ConsoleState {
    pub(crate) fn new() -> Self {
        Self {
            mode: FrontendMode::Idle,
            pending_approval: None,
        }
    }

    pub(crate) fn mode(&self) -> FrontendMode {
        self.mode
    }

    pub(crate) fn can_submit_turn(&self) -> bool {
        self.mode == FrontendMode::Idle
    }

    pub(crate) fn take_pending_approval(&mut self) -> Option<PendingApproval> {
        self.pending_approval.take()
    }

    pub(crate) fn update_from_protocol(&mut self, event: &AppServerEvent) {
        match event {
            AppServerEvent::FrontendStateChanged { mode, .. } => {
                self.mode = *mode;
                if *mode != FrontendMode::WaitingForApproval {
                    self.pending_approval = None;
                }
            }
            AppServerEvent::TurnFinished { .. } => {
                self.mode = FrontendMode::Idle;
                self.pending_approval = None;
            }
            _ => {}
        }
    }

    pub(crate) fn set_pending_approval(
        &mut self,
        _request: ApprovalRequest,
        reply: oneshot::Sender<ApprovalDecision>,
    ) {
        self.mode = FrontendMode::WaitingForApproval;
        self.pending_approval = Some(PendingApproval { reply });
    }
}
