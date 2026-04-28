use agent_protocol::{AppServerEvent, ApprovalDecision, ApprovalRequest, TurnEvent};
use tokio::sync::oneshot;

#[derive(Debug)]
pub(crate) enum ControllerEvent {
    Protocol(AppServerEvent),
    Runtime(TurnEvent),
    ApprovalRequest {
        request: ApprovalRequest,
        reply: oneshot::Sender<ApprovalDecision>,
    },
}
