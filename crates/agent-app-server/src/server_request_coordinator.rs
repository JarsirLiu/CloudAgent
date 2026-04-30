use agent_protocol::{RequestId, ServerRequest, ServerRequestDecision, TurnId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::oneshot;

#[derive(Debug)]
pub(crate) struct ServerRequestCoordinator {
    pending: HashMap<RequestId, PendingServerRequest>,
    request_counter: AtomicI64,
}

#[derive(Debug)]
pub(crate) struct PendingServerRequest {
    pub(crate) turn_id: TurnId,
    pub(crate) request: ServerRequest,
    pub(crate) reply_tx: oneshot::Sender<ServerRequestDecision>,
}

impl ServerRequestCoordinator {
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
        turn_id: TurnId,
        request: ServerRequest,
        reply_tx: oneshot::Sender<ServerRequestDecision>,
    ) {
        self.pending.insert(
            request_id,
            PendingServerRequest {
                turn_id,
                request,
                reply_tx,
            },
        );
    }

    pub(crate) fn resolve(
        &mut self,
        request_id: &RequestId,
        decision: ServerRequestDecision,
    ) -> Option<(TurnId, ServerRequest, ServerRequestDecision)> {
        if let Some(pending) = self.pending.remove(request_id) {
            let _ = pending.reply_tx.send(decision.clone());
            Some((pending.turn_id, pending.request, decision))
        } else {
            None
        }
    }

    pub(crate) fn drain_turn(
        &mut self,
        turn_id: &str,
        decision: ServerRequestDecision,
    ) -> Vec<(RequestId, TurnId, ServerRequest, ServerRequestDecision)> {
        let request_ids = self
            .pending
            .iter()
            .filter_map(|(request_id, pending)| {
                (pending.turn_id == turn_id).then_some(request_id.clone())
            })
            .collect::<Vec<_>>();
        request_ids
            .into_iter()
            .filter_map(|request_id| {
                self.pending.remove(&request_id).map(|pending| {
                    let _ = pending.reply_tx.send(decision.clone());
                    (
                        request_id,
                        pending.turn_id,
                        pending.request,
                        decision.clone(),
                    )
                })
            })
            .collect()
    }
}
