use agent_core::conversation::ConversationTurn;
use agent_protocol::{
    ConversationActiveFlag, ConversationViewSnapshot, ConversationViewStatus,
    PendingServerRequestView, RequestId, TurnViewStatus,
};
use std::collections::HashMap;
use tokio::sync::watch;

#[derive(Default)]
pub(crate) struct ConversationRuntimeViewManager {
    conversations: HashMap<String, ConversationRuntimeEntry>,
}

pub(crate) enum ConversationRuntimeUpdate {
    MarkLoaded {
        conversation_id: String,
    },
    UpdateMessageCount {
        conversation_id: String,
        message_count: usize,
    },
    TurnStarting {
        conversation_id: String,
    },
    TurnStarted {
        conversation_id: String,
        turn_id: String,
    },
    UpdateActiveTurn {
        conversation_id: String,
        turn: Option<ConversationTurn>,
    },
    TurnFinished {
        conversation_id: String,
        final_status: TurnViewStatus,
    },
    RequestPending {
        conversation_id: String,
        request: PendingServerRequestView,
    },
    RequestResolved {
        conversation_id: String,
        request_id: RequestId,
    },
    #[allow(dead_code)]
    UserInputRequested {
        conversation_id: String,
    },
    UserInputResolved {
        conversation_id: String,
    },
    InterruptRequested {
        conversation_id: String,
    },
    CompactionStarted {
        conversation_id: String,
    },
    CompactionFinished {
        conversation_id: String,
    },
    SystemError {
        conversation_id: String,
        message: String,
    },
}

#[derive(Clone, Debug)]
struct ConversationRuntimeFacts {
    conversation_id: String,
    loaded: bool,
    running: bool,
    active_turn_id: Option<String>,
    active_turn: Option<ConversationTurn>,
    active_turn_status: Option<TurnViewStatus>,
    pending_requests: Vec<PendingServerRequestView>,
    waiting_on_user_input_count: u32,
    interrupt_requested: bool,
    compacting_context: bool,
    system_error: Option<String>,
    message_count: usize,
    updated_at_ms: u64,
}

struct ConversationRuntimeEntry {
    facts: ConversationRuntimeFacts,
    watch_tx: watch::Sender<ConversationViewSnapshot>,
}

#[allow(dead_code)]
pub(crate) type ConversationRuntimeWatch = watch::Receiver<ConversationViewSnapshot>;

impl ConversationRuntimeViewManager {
    pub(crate) fn snapshot(&self, conversation_id: &str) -> ConversationViewSnapshot {
        self.conversations
            .get(conversation_id)
            .map(|entry| entry.facts.snapshot())
            .unwrap_or_else(|| ConversationRuntimeFacts::new(conversation_id).snapshot())
    }

    #[allow(dead_code)]
    pub(crate) fn subscribe(&mut self, conversation_id: &str) -> ConversationRuntimeWatch {
        self.entry_mut(conversation_id).watch_tx.subscribe()
    }

    pub(crate) fn apply(&mut self, update: ConversationRuntimeUpdate) -> ConversationViewSnapshot {
        let conversation_id = update.conversation_id().to_string();
        match update {
            ConversationRuntimeUpdate::MarkLoaded { conversation_id } => {
                self.mark_loaded(conversation_id);
            }
            ConversationRuntimeUpdate::UpdateMessageCount {
                conversation_id,
                message_count,
            } => {
                self.update_message_count(&conversation_id, message_count);
            }
            ConversationRuntimeUpdate::TurnStarting { conversation_id } => {
                self.turn_starting(&conversation_id);
            }
            ConversationRuntimeUpdate::TurnStarted {
                conversation_id,
                turn_id,
            } => {
                self.turn_started(&conversation_id, turn_id);
            }
            ConversationRuntimeUpdate::UpdateActiveTurn {
                conversation_id,
                turn,
            } => {
                self.update_active_turn(&conversation_id, turn);
            }
            ConversationRuntimeUpdate::TurnFinished {
                conversation_id,
                final_status,
            } => {
                self.turn_finished(&conversation_id, final_status);
            }
            ConversationRuntimeUpdate::RequestPending {
                conversation_id,
                request,
            } => {
                self.request_pending(&conversation_id, request);
            }
            ConversationRuntimeUpdate::RequestResolved {
                conversation_id,
                request_id,
            } => {
                self.request_resolved(&conversation_id, &request_id);
            }
            ConversationRuntimeUpdate::UserInputRequested { conversation_id } => {
                self.user_input_requested(&conversation_id);
            }
            ConversationRuntimeUpdate::UserInputResolved { conversation_id } => {
                self.user_input_resolved(&conversation_id);
            }
            ConversationRuntimeUpdate::InterruptRequested { conversation_id } => {
                self.interrupt_requested(&conversation_id);
            }
            ConversationRuntimeUpdate::CompactionStarted { conversation_id } => {
                self.compaction_started(&conversation_id);
            }
            ConversationRuntimeUpdate::CompactionFinished { conversation_id } => {
                self.compaction_finished(&conversation_id);
            }
            ConversationRuntimeUpdate::SystemError {
                conversation_id,
                message,
            } => {
                self.system_error(&conversation_id, message);
            }
        }
        self.publish_snapshot(&conversation_id)
    }

    pub(crate) fn mark_loaded(&mut self, conversation_id: impl Into<String>) {
        let conversation_id = conversation_id.into();
        let facts = self.facts_mut(&conversation_id);
        facts.loaded = true;
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn update_message_count(&mut self, conversation_id: &str, message_count: usize) {
        let facts = self.facts_mut(conversation_id);
        facts.loaded = true;
        facts.message_count = message_count;
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn turn_starting(&mut self, conversation_id: &str) {
        let facts = self.facts_mut(conversation_id);
        facts.loaded = true;
        facts.running = true;
        facts.active_turn_id = None;
        facts.active_turn = None;
        facts.active_turn_status = Some(TurnViewStatus::InProgress);
        facts.pending_requests.clear();
        facts.waiting_on_user_input_count = 0;
        facts.interrupt_requested = false;
        facts.system_error = None;
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn turn_started(&mut self, conversation_id: &str, turn_id: String) {
        let facts = self.facts_mut(conversation_id);
        facts.loaded = true;
        facts.running = true;
        facts.active_turn_id = Some(turn_id);
        facts.active_turn_status = Some(TurnViewStatus::InProgress);
        facts.waiting_on_user_input_count = 0;
        facts.interrupt_requested = false;
        facts.system_error = None;
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn update_active_turn(
        &mut self,
        conversation_id: &str,
        turn: Option<ConversationTurn>,
    ) {
        let facts = self.facts_mut(conversation_id);
        facts.loaded = true;
        facts.active_turn = turn;
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn turn_finished(&mut self, conversation_id: &str, final_status: TurnViewStatus) {
        let facts = self.facts_mut(conversation_id);
        facts.loaded = true;
        facts.running = false;
        facts.active_turn_id = None;
        facts.active_turn = None;
        facts.active_turn_status = Some(final_status);
        facts.pending_requests.clear();
        facts.waiting_on_user_input_count = 0;
        facts.interrupt_requested = false;
        facts.compacting_context = false;
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn request_pending(
        &mut self,
        conversation_id: &str,
        request: PendingServerRequestView,
    ) {
        let facts = self.facts_mut(conversation_id);
        facts.loaded = true;
        facts.running = true;
        facts
            .pending_requests
            .retain(|pending| pending.request_id != request.request_id);
        facts.pending_requests.push(request);
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn request_resolved(&mut self, conversation_id: &str, request_id: &RequestId) {
        let facts = self.facts_mut(conversation_id);
        let previous_len = facts.pending_requests.len();
        facts
            .pending_requests
            .retain(|pending| &pending.request_id != request_id);
        if facts.pending_requests.len() != previous_len {
            facts.updated_at_ms = next_updated_at_ms();
        }
    }

    pub(crate) fn user_input_requested(&mut self, conversation_id: &str) {
        let facts = self.facts_mut(conversation_id);
        facts.loaded = true;
        facts.running = true;
        facts.waiting_on_user_input_count = facts.waiting_on_user_input_count.saturating_add(1);
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn user_input_resolved(&mut self, conversation_id: &str) {
        let facts = self.facts_mut(conversation_id);
        let previous = facts.waiting_on_user_input_count;
        facts.waiting_on_user_input_count = facts.waiting_on_user_input_count.saturating_sub(1);
        if facts.waiting_on_user_input_count != previous {
            facts.updated_at_ms = next_updated_at_ms();
        }
    }

    pub(crate) fn interrupt_requested(&mut self, conversation_id: &str) {
        let facts = self.facts_mut(conversation_id);
        facts.loaded = true;
        facts.interrupt_requested = true;
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn compaction_started(&mut self, conversation_id: &str) {
        let facts = self.facts_mut(conversation_id);
        facts.loaded = true;
        facts.compacting_context = true;
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn compaction_finished(&mut self, conversation_id: &str) {
        let facts = self.facts_mut(conversation_id);
        facts.compacting_context = false;
        facts.updated_at_ms = next_updated_at_ms();
    }

    pub(crate) fn system_error(&mut self, conversation_id: &str, message: String) {
        let facts = self.facts_mut(conversation_id);
        facts.loaded = true;
        facts.running = false;
        facts.active_turn_id = None;
        facts.active_turn = None;
        facts.active_turn_status = Some(TurnViewStatus::Failed);
        facts.pending_requests.clear();
        facts.waiting_on_user_input_count = 0;
        facts.interrupt_requested = false;
        facts.compacting_context = false;
        facts.system_error = Some(message);
        facts.updated_at_ms = next_updated_at_ms();
    }

    fn facts_mut(&mut self, conversation_id: &str) -> &mut ConversationRuntimeFacts {
        &mut self.entry_mut(conversation_id).facts
    }

    fn entry_mut(&mut self, conversation_id: &str) -> &mut ConversationRuntimeEntry {
        self.conversations
            .entry(conversation_id.to_string())
            .or_insert_with(|| ConversationRuntimeEntry::new(conversation_id))
    }

    fn publish_snapshot(&mut self, conversation_id: &str) -> ConversationViewSnapshot {
        let entry = self.entry_mut(conversation_id);
        let snapshot = entry.facts.snapshot();
        entry.watch_tx.send_replace(snapshot.clone());
        snapshot
    }
}

impl ConversationRuntimeUpdate {
    pub(crate) fn conversation_id(&self) -> &str {
        match self {
            ConversationRuntimeUpdate::MarkLoaded { conversation_id }
            | ConversationRuntimeUpdate::UpdateMessageCount {
                conversation_id, ..
            }
            | ConversationRuntimeUpdate::TurnStarting { conversation_id }
            | ConversationRuntimeUpdate::TurnStarted {
                conversation_id, ..
            }
            | ConversationRuntimeUpdate::UpdateActiveTurn {
                conversation_id, ..
            }
            | ConversationRuntimeUpdate::TurnFinished {
                conversation_id, ..
            }
            | ConversationRuntimeUpdate::RequestPending {
                conversation_id, ..
            }
            | ConversationRuntimeUpdate::RequestResolved {
                conversation_id, ..
            }
            | ConversationRuntimeUpdate::UserInputRequested { conversation_id }
            | ConversationRuntimeUpdate::UserInputResolved { conversation_id }
            | ConversationRuntimeUpdate::InterruptRequested { conversation_id }
            | ConversationRuntimeUpdate::CompactionStarted { conversation_id }
            | ConversationRuntimeUpdate::CompactionFinished { conversation_id }
            | ConversationRuntimeUpdate::SystemError {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

impl ConversationRuntimeFacts {
    fn new(conversation_id: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            loaded: false,
            running: false,
            active_turn_id: None,
            active_turn: None,
            active_turn_status: None,
            pending_requests: Vec::new(),
            waiting_on_user_input_count: 0,
            interrupt_requested: false,
            compacting_context: false,
            system_error: None,
            message_count: 0,
            updated_at_ms: 0,
        }
    }

    fn snapshot(&self) -> ConversationViewSnapshot {
        ConversationViewSnapshot {
            conversation_id: self.conversation_id.clone(),
            status: self.status(),
            active_turn: self.active_turn.clone(),
            pending_requests: self.pending_requests.clone(),
            message_count: self.message_count,
            updated_at_ms: self.updated_at_ms,
        }
    }

    fn status(&self) -> ConversationViewStatus {
        if !self.loaded {
            return ConversationViewStatus::NotLoaded;
        }

        if let Some(message) = self.system_error.clone() {
            return ConversationViewStatus::SystemError { message };
        }

        if !self.running {
            return ConversationViewStatus::Idle;
        }

        let mut flags = vec![ConversationActiveFlag::RunningTurn];
        if !self.pending_requests.is_empty() {
            flags.push(ConversationActiveFlag::WaitingOnApproval);
        }
        if self.waiting_on_user_input_count > 0 {
            flags.push(ConversationActiveFlag::WaitingOnUserInput);
        }
        if self.interrupt_requested {
            flags.push(ConversationActiveFlag::InterruptRequested);
        }
        if self.compacting_context {
            flags.push(ConversationActiveFlag::CompactingContext);
        }

        ConversationViewStatus::Active {
            active_turn_id: self.active_turn_id.clone(),
            flags,
        }
    }
}

impl ConversationRuntimeEntry {
    fn new(conversation_id: impl Into<String>) -> Self {
        let facts = ConversationRuntimeFacts::new(conversation_id);
        let snapshot = facts.snapshot();
        let (watch_tx, _) = watch::channel(snapshot);
        Self { facts, watch_tx }
    }
}

fn next_updated_at_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "conversation_runtime_tests.rs"]
mod conversation_runtime_tests;
