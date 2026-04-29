use agent_protocol::{ApprovalDecision, RequestId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::oneshot;

#[derive(Debug)]
pub(crate) struct ApprovalCoordinator {
    pending: HashMap<RequestId, oneshot::Sender<ApprovalDecision>>,
    request_counter: AtomicI64,
}

impl ApprovalCoordinator {
    pub(crate) fn new() -> Self {
        Self {
            pending: HashMap::new(),
            request_counter: AtomicI64::new(1),
        }
    }

    pub(crate) fn next_request_id(&self) -> RequestId {
        RequestId::Integer(self.request_counter.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn insert_pending(
        &mut self,
        request_id: RequestId,
        reply_tx: oneshot::Sender<ApprovalDecision>,
    ) {
        self.pending.insert(request_id, reply_tx);
    }

    pub(crate) fn resolve(
        &mut self,
        request_id: &RequestId,
        decision: ApprovalDecision,
    ) -> bool {
        if let Some(reply) = self.pending.remove(request_id) {
            let _ = reply.send(decision);
            true
        } else {
            false
        }
    }
}

