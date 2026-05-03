use agent_protocol::{RequestId, ServerRequest, ServerRequestDecision, TurnId};
use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::oneshot;

#[derive(Debug)]
pub(crate) struct ServerRequestCoordinator {
    pending: HashMap<RequestId, PendingServerRequest>,
    resolved: HashMap<RequestId, ResolvedServerRequest>,
    request_counter: AtomicI64,
}

#[derive(Debug)]
pub(crate) struct PendingServerRequest {
    pub(crate) conversation_id: String,
    pub(crate) turn_id: TurnId,
    pub(crate) request: ServerRequest,
    pub(crate) reply_tx: oneshot::Sender<ServerRequestDecision>,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedServerRequest {
    pub(crate) conversation_id: String,
    pub(crate) turn_id: TurnId,
    pub(crate) request: ServerRequest,
    pub(crate) decision: ServerRequestDecision,
}

impl ServerRequestCoordinator {
    pub(crate) fn new() -> Self {
        Self {
            pending: HashMap::new(),
            resolved: HashMap::new(),
            request_counter: AtomicI64::new(1),
        }
    }

    pub(crate) fn next_request_id(&self) -> RequestId {
        RequestId::Integer(self.request_counter.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn insert_pending(
        &mut self,
        request_id: RequestId,
        conversation_id: String,
        turn_id: TurnId,
        request: ServerRequest,
        reply_tx: oneshot::Sender<ServerRequestDecision>,
    ) {
        self.pending.insert(
            request_id,
            PendingServerRequest {
                conversation_id,
                turn_id,
                request,
                reply_tx,
            },
        );
    }

    pub(crate) fn pending_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Vec<(RequestId, ServerRequest)> {
        let mut requests = self
            .pending
            .iter()
            .filter_map(|(request_id, pending)| {
                (pending.conversation_id == conversation_id)
                    .then_some((request_id.clone(), pending.request.clone()))
            })
            .collect::<Vec<_>>();
        requests.sort_by(|(left, _), (right, _)| request_id_order(left, right));
        requests
    }

    pub(crate) fn resolve(
        &mut self,
        request_id: &RequestId,
        decision: ServerRequestDecision,
    ) -> Option<ResolvedServerRequest> {
        if let Some(pending) = self.pending.remove(request_id) {
            let _ = pending.reply_tx.send(decision.clone());
            let resolved = ResolvedServerRequest {
                conversation_id: pending.conversation_id,
                turn_id: pending.turn_id,
                request: pending.request,
                decision,
            };
            self.resolved.insert(request_id.clone(), resolved.clone());
            Some(resolved)
        } else {
            self.resolved.get(request_id).cloned()
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

    pub(crate) fn drain_conversation(
        &mut self,
        conversation_id: &str,
        decision: ServerRequestDecision,
    ) -> Vec<(RequestId, TurnId, ServerRequest, ServerRequestDecision)> {
        let request_ids = self
            .pending
            .iter()
            .filter_map(|(request_id, pending)| {
                (pending.conversation_id == conversation_id).then_some(request_id.clone())
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

fn request_id_order(left: &RequestId, right: &RequestId) -> CmpOrdering {
    match (left, right) {
        (RequestId::Integer(a), RequestId::Integer(b)) => a.cmp(b),
        (RequestId::String(a), RequestId::String(b)) => a.cmp(b),
        (RequestId::Integer(_), RequestId::String(_)) => CmpOrdering::Less,
        (RequestId::String(_), RequestId::Integer(_)) => CmpOrdering::Greater,
    }
}

#[cfg(test)]
mod tests {
    use super::ServerRequestCoordinator;
    use agent_protocol::{
        RequestId, ServerRequest, ServerRequestDecision, ToolApprovalRequest,
    };
    use tokio::sync::oneshot;

    #[test]
    fn resolve_is_idempotent_for_same_request_id() {
        let mut coordinator = ServerRequestCoordinator::new();
        let (tx, _rx) = oneshot::channel();
        let request_id = RequestId::Integer(7);
        let decision = ServerRequestDecision::accept(Some("ok".to_string()));
        coordinator.insert_pending(
            request_id.clone(),
            "conv-a".to_string(),
            "turn-1".to_string(),
            ServerRequest::ToolApproval {
                request: ToolApprovalRequest {
                    turn_id: "turn-1".to_string(),
                    tool_call_id: "call-1".to_string(),
                    tool_name: "exec_command".to_string(),
                    reason: "test".to_string(),
                    arguments_preview: "{}".to_string(),
                },
            },
            tx,
        );

        let first = coordinator
            .resolve(&request_id, decision.clone())
            .expect("first resolve");
        let second = coordinator
            .resolve(&request_id, decision)
            .expect("second resolve should be replayed");

        assert_eq!(first.conversation_id, "conv-a");
        assert_eq!(second.conversation_id, "conv-a");
        assert_eq!(first.turn_id, "turn-1");
        assert_eq!(second.turn_id, "turn-1");
    }

    #[test]
    fn pending_for_conversation_is_stably_ordered_by_request_id() {
        let mut coordinator = ServerRequestCoordinator::new();
        for request_id in [
            RequestId::Integer(10),
            RequestId::Integer(2),
            RequestId::String("b".to_string()),
            RequestId::String("a".to_string()),
        ] {
            let (tx, _rx) = oneshot::channel();
            coordinator.insert_pending(
                request_id,
                "conv-a".to_string(),
                "turn-1".to_string(),
                ServerRequest::ToolApproval {
                    request: ToolApprovalRequest {
                        turn_id: "turn-1".to_string(),
                        tool_call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        reason: "test".to_string(),
                        arguments_preview: "{}".to_string(),
                    },
                },
                tx,
            );
        }
        let ids = coordinator
            .pending_for_conversation("conv-a")
            .into_iter()
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                RequestId::Integer(2),
                RequestId::Integer(10),
                RequestId::String("a".to_string()),
                RequestId::String("b".to_string()),
            ]
        );
    }
}

