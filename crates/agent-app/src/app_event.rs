use crate::input::ParsedInput;
use agent_protocol::{AppServerEvent, ApprovalDecision, ApprovalRequest, TurnEvent};
use tokio::sync::oneshot;

#[derive(Debug)]
pub(crate) enum AppEvent {
    Input(ParsedInput),
    RuntimeEvent(TurnEvent),
    ProtocolEvent(AppServerEvent),
    ApprovalRequest {
        request: ApprovalRequest,
        reply: oneshot::Sender<ApprovalDecision>,
    },
}
