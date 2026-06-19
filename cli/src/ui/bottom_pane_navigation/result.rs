use crate::input::intent::ComposerIntent;
use agent_core::ServerRequestDecisionKind;
use agent_protocol::RequestId;

pub(crate) enum NavigationKeyResult {
    NoActiveView,
    Handled,
    Composer(ComposerIntent),
    LoadMoreSessions {
        cursor: String,
    },
    ServerRequestSubmit {
        request_id: RequestId,
        decision: ServerRequestDecisionKind,
        reason: String,
    },
}
