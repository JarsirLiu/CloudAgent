use crate::app::notification::send_notification;
use crate::routing::command_router::ServerState;
use crate::session::conversation_runtime::{
    ConversationRuntimeUpdate, ConversationRuntimeViewManager, ConversationRuntimeWatch,
};
use agent_core::ConversationTurn;
use agent_protocol::{
    AppServerMessage, AppServerNotification, ConversationViewSnapshot, PendingServerRequestView,
    RequestId, TurnViewStatus,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, mpsc};

#[derive(Clone)]
pub(crate) struct ConversationWatchManager {
    inner: Arc<Mutex<ConversationWatchState>>,
    event_tx: mpsc::UnboundedSender<AppServerMessage>,
    server_state: Arc<Mutex<ServerState>>,
}

struct ConversationWatchState {
    runtime: ConversationRuntimeViewManager,
}

struct ConversationViewMutation {
    conversation_id: String,
    snapshot: ConversationViewSnapshot,
    changed: bool,
}

pub(crate) struct ConversationRuntimeActiveGuard {
    manager: ConversationWatchManager,
    conversation_id: String,
    guard_type: ConversationRuntimeActiveGuardType,
    released: Arc<AtomicBool>,
    handle: tokio::runtime::Handle,
}

#[derive(Clone)]
enum ConversationRuntimeActiveGuardType {
    ServerRequest {
        request_id: RequestId,
    },
    #[allow(dead_code)]
    UserInput,
}

impl ConversationWatchManager {
    pub(crate) fn new(
        event_tx: mpsc::UnboundedSender<AppServerMessage>,
        server_state: Arc<Mutex<ServerState>>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ConversationWatchState {
                runtime: ConversationRuntimeViewManager::default(),
            })),
            event_tx,
            server_state,
        }
    }

    pub(crate) async fn snapshot(&self, conversation_id: &str) -> ConversationViewSnapshot {
        self.inner.lock().await.runtime.snapshot(conversation_id)
    }

    pub(crate) fn event_tx(&self) -> &mpsc::UnboundedSender<AppServerMessage> {
        &self.event_tx
    }

    pub(crate) fn server_state(&self) -> &Arc<Mutex<ServerState>> {
        &self.server_state
    }

    #[allow(dead_code)]
    pub(crate) async fn subscribe(&self, conversation_id: &str) -> ConversationRuntimeWatch {
        self.inner.lock().await.runtime.subscribe(conversation_id)
    }

    pub(crate) async fn emit_current(&self, conversation_id: &str) {
        let snapshot = self.snapshot(conversation_id).await;
        send_notification(
            &self.event_tx,
            &self.server_state,
            AppServerNotification::ConversationViewChanged {
                conversation_id: conversation_id.to_string(),
                snapshot,
            },
        )
        .await;
    }

    pub(crate) async fn apply(&self, update: ConversationRuntimeUpdate) {
        let mutation = self.apply_update(update).await;
        self.publish_if_changed(mutation).await;
    }

    pub(crate) async fn note_loaded(&self, conversation_id: &str, message_count: usize) {
        self.apply(ConversationRuntimeUpdate::MarkLoaded {
            conversation_id: conversation_id.to_string(),
        })
        .await;
        self.apply(ConversationRuntimeUpdate::UpdateMessageCount {
            conversation_id: conversation_id.to_string(),
            message_count,
        })
        .await;
    }

    pub(crate) async fn note_turn_starting(&self, conversation_id: &str) {
        self.apply(ConversationRuntimeUpdate::TurnStarting {
            conversation_id: conversation_id.to_string(),
        })
        .await;
    }

    pub(crate) async fn note_turn_started(&self, conversation_id: &str, turn_id: String) {
        self.apply(ConversationRuntimeUpdate::TurnStarted {
            conversation_id: conversation_id.to_string(),
            turn_id,
        })
        .await;
    }

    pub(crate) async fn note_active_turn_snapshot(
        &self,
        conversation_id: &str,
        turn: Option<ConversationTurn>,
    ) {
        self.apply(ConversationRuntimeUpdate::UpdateActiveTurn {
            conversation_id: conversation_id.to_string(),
            turn,
        })
        .await;
    }

    pub(crate) async fn note_turn_finished(
        &self,
        conversation_id: &str,
        final_status: TurnViewStatus,
    ) {
        self.apply(ConversationRuntimeUpdate::TurnFinished {
            conversation_id: conversation_id.to_string(),
            final_status,
        })
        .await;
    }

    pub(crate) async fn note_interrupt_requested(&self, conversation_id: &str) {
        self.apply(ConversationRuntimeUpdate::InterruptRequested {
            conversation_id: conversation_id.to_string(),
        })
        .await;
    }

    pub(crate) async fn note_compaction_started(&self, conversation_id: &str) {
        self.apply(ConversationRuntimeUpdate::CompactionStarted {
            conversation_id: conversation_id.to_string(),
        })
        .await;
    }

    pub(crate) async fn note_compaction_finished(&self, conversation_id: &str) {
        self.apply(ConversationRuntimeUpdate::CompactionFinished {
            conversation_id: conversation_id.to_string(),
        })
        .await;
    }

    pub(crate) async fn note_system_error(&self, conversation_id: &str, message: String) {
        self.apply(ConversationRuntimeUpdate::SystemError {
            conversation_id: conversation_id.to_string(),
            message,
        })
        .await;
    }

    pub(crate) async fn note_server_request_pending(
        &self,
        conversation_id: &str,
        request: PendingServerRequestView,
    ) -> ConversationRuntimeActiveGuard {
        let request_id = request.request_id.clone();
        self.apply(ConversationRuntimeUpdate::RequestPending {
            conversation_id: conversation_id.to_string(),
            request,
        })
        .await;
        ConversationRuntimeActiveGuard::new(
            self.clone(),
            conversation_id.to_string(),
            ConversationRuntimeActiveGuardType::ServerRequest { request_id },
        )
    }

    pub(crate) async fn note_server_request_resolved(
        &self,
        conversation_id: &str,
        request_id: RequestId,
    ) {
        self.apply(ConversationRuntimeUpdate::RequestResolved {
            conversation_id: conversation_id.to_string(),
            request_id,
        })
        .await;
    }

    #[allow(dead_code)]
    pub(crate) async fn note_user_input_requested(
        &self,
        conversation_id: &str,
    ) -> ConversationRuntimeActiveGuard {
        self.apply(ConversationRuntimeUpdate::UserInputRequested {
            conversation_id: conversation_id.to_string(),
        })
        .await;
        ConversationRuntimeActiveGuard::new(
            self.clone(),
            conversation_id.to_string(),
            ConversationRuntimeActiveGuardType::UserInput,
        )
    }

    async fn note_active_guard_released(
        &self,
        conversation_id: String,
        guard_type: ConversationRuntimeActiveGuardType,
    ) {
        match guard_type {
            ConversationRuntimeActiveGuardType::ServerRequest { request_id } => {
                self.note_server_request_resolved(&conversation_id, request_id)
                    .await;
            }
            ConversationRuntimeActiveGuardType::UserInput => {
                self.apply(ConversationRuntimeUpdate::UserInputResolved { conversation_id })
                    .await;
            }
        }
    }

    async fn apply_update(&self, update: ConversationRuntimeUpdate) -> ConversationViewMutation {
        let conversation_id = update.conversation_id().to_string();
        let mut state = self.inner.lock().await;
        let previous = state.runtime.snapshot(&conversation_id);
        let snapshot = state.runtime.apply(update);
        let changed = !snapshots_equivalent_for_publish(&previous, &snapshot);
        ConversationViewMutation {
            conversation_id,
            snapshot,
            changed,
        }
    }

    async fn publish_if_changed(&self, mutation: ConversationViewMutation) {
        if !mutation.changed {
            return;
        }
        send_notification(
            &self.event_tx,
            &self.server_state,
            AppServerNotification::ConversationViewChanged {
                conversation_id: mutation.conversation_id,
                snapshot: mutation.snapshot,
            },
        )
        .await;
    }
}

impl ConversationRuntimeActiveGuard {
    fn new(
        manager: ConversationWatchManager,
        conversation_id: String,
        guard_type: ConversationRuntimeActiveGuardType,
    ) -> Self {
        Self {
            manager,
            conversation_id,
            guard_type,
            released: Arc::new(AtomicBool::new(false)),
            handle: tokio::runtime::Handle::current(),
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn release(&self) {
        if self.released.swap(true, Ordering::SeqCst) {
            return;
        }
        self.manager
            .note_active_guard_released(self.conversation_id.clone(), self.guard_type.clone())
            .await;
    }
}

impl Drop for ConversationRuntimeActiveGuard {
    fn drop(&mut self) {
        if self.released.swap(true, Ordering::SeqCst) {
            return;
        }
        let manager = self.manager.clone();
        let conversation_id = self.conversation_id.clone();
        let guard_type = self.guard_type.clone();
        self.handle.spawn(async move {
            manager
                .note_active_guard_released(conversation_id, guard_type)
                .await;
        });
    }
}

fn snapshots_equivalent_for_publish(
    previous: &ConversationViewSnapshot,
    current: &ConversationViewSnapshot,
) -> bool {
    previous.conversation_id == current.conversation_id
        && previous.status == current.status
        && same_active_turn(previous.active_turn.as_ref(), current.active_turn.as_ref())
        && same_pending_requests(&previous.pending_requests, &current.pending_requests)
        && previous.message_count == current.message_count
}

fn same_active_turn(left: Option<&ConversationTurn>, right: Option<&ConversationTurn>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.id == right.id
                && left.state == right.state
                && left.items.len() == right.items.len()
                && left.rollout_start_index == right.rollout_start_index
                && left.rollout_end_index == right.rollout_end_index
        }
        _ => false,
    }
}

fn same_pending_requests(
    left: &[PendingServerRequestView],
    right: &[PendingServerRequestView],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right.iter()).all(|(left, right)| {
            left.request_id == right.request_id
                && left.conversation_id == right.conversation_id
                && left.turn_id == right.turn_id
                && left.kind == right.kind
                && left.tool_name == right.tool_name
                && left.reason == right.reason
                && left.preview == right.preview
        })
}

#[cfg(test)]
#[path = "conversation_watch_tests.rs"]
mod conversation_watch_tests;
