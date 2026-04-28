use crate::app_event::AppEvent;
use crate::input::ParsedInput;
use agent_protocol::{AppServerEvent, ApprovalDecision, ApprovalRequest, TurnEvent};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

#[derive(Clone, Debug)]
pub(crate) struct AppEventSender {
    tx: UnboundedSender<AppEvent>,
}

impl AppEventSender {
    pub(crate) fn new(tx: UnboundedSender<AppEvent>) -> Self {
        Self { tx }
    }

    pub(crate) fn send(&self, event: AppEvent) {
        let _ = self.tx.send(event);
    }

    pub(crate) fn input(&self, input: ParsedInput) {
        self.send(AppEvent::Input(input));
    }

    pub(crate) fn runtime_event(&self, event: TurnEvent) {
        self.send(AppEvent::RuntimeEvent(event));
    }

    pub(crate) fn protocol_event(&self, event: AppServerEvent) {
        self.send(AppEvent::ProtocolEvent(event));
    }

    pub(crate) fn approval_request(
        &self,
        request: ApprovalRequest,
        reply: oneshot::Sender<ApprovalDecision>,
    ) {
        self.send(AppEvent::ApprovalRequest { request, reply });
    }
}
