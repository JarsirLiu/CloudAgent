use crate::input::intent::ComposerIntent;
use agent_core::ServerRequestDecisionKind;
use agent_protocol::RequestId;

pub(crate) enum NavigationKeyResult {
    NoActiveView,
    Consumed,
    Composer(ComposerIntent),
    ServerRequestSubmit {
        request_id: RequestId,
        decision: ServerRequestDecisionKind,
        reason: String,
    },
    FallthroughEscFromActionRequiredView,
}
