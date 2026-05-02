use crate::conversation_listener::ConversationListenerHandle;
use crate::conversation_subscriptions::ConversationSubscriptions;
use crate::conversation_service;
use crate::server_request_service;
use crate::server_request_coordinator::ServerRequestCoordinator;
use crate::turn_service;
use agent_core::ConversationTurn;
use agent_protocol::{AppClientCommand, AppServerMessage, ServerRequestDecision};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;

pub(crate) struct ServerState {
    active_conversation_id: String,
    subscriptions: ConversationSubscriptions,
    server_requests: ServerRequestCoordinator,
    turn_tasks: Vec<JoinHandle<()>>,
    active_listeners: HashMap<String, ConversationListenerHandle>,
}

impl ServerState {
    pub(crate) fn new(default_conversation_id: String) -> Self {
        Self {
            active_conversation_id: default_conversation_id.clone(),
            subscriptions: ConversationSubscriptions::new(default_conversation_id),
            server_requests: ServerRequestCoordinator::new(),
            turn_tasks: Vec::new(),
            active_listeners: HashMap::new(),
        }
    }

    pub(crate) fn track_turn_task(&mut self, task: JoinHandle<()>) {
        self.turn_tasks.retain(|task| !task.is_finished());
        self.turn_tasks.push(task);
    }

    pub(crate) fn take_turn_tasks(&mut self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut self.turn_tasks)
    }

    pub(crate) fn set_active_listener(
        &mut self,
        conversation_id: String,
        listener: ConversationListenerHandle,
    ) {
        self.active_listeners.insert(conversation_id, listener);
    }

    pub(crate) fn clear_active_listener(&mut self, conversation_id: &str) {
        self.active_listeners.remove(conversation_id);
    }

    pub(crate) fn active_listener(
        &self,
        conversation_id: &str,
    ) -> Option<ConversationListenerHandle> {
        self.active_listeners.get(conversation_id).cloned()
    }

    pub(crate) fn active_conversation_id(&self) -> &str {
        &self.active_conversation_id
    }

    pub(crate) fn switch_active_conversation(&mut self, conversation_id: String) {
        self.active_conversation_id = conversation_id;
    }

    pub(crate) fn is_subscribed(&self, conversation_id: &str) -> bool {
        self.subscriptions.is_subscribed(conversation_id)
    }

    pub(crate) fn subscribe(&mut self, conversation_id: String) {
        self.subscriptions.subscribe(conversation_id);
    }

    pub(crate) fn unsubscribe(&mut self, conversation_id: &str) {
        self.subscriptions.unsubscribe(conversation_id);
    }

    pub(crate) fn resolve_server_request(
        &mut self,
        request_id: &agent_protocol::RequestId,
        decision: ServerRequestDecision,
    ) -> Option<(
        String,
        agent_protocol::ServerRequest,
        ServerRequestDecision,
    )> {
        self.server_requests.resolve(request_id, decision)
    }

    pub(crate) fn drain_server_requests_for_turn(
        &mut self,
        turn_id: &str,
        decision: ServerRequestDecision,
    ) -> Vec<(
        agent_protocol::RequestId,
        String,
        agent_protocol::ServerRequest,
        ServerRequestDecision,
    )> {
        self.server_requests.drain_turn(turn_id, decision)
    }

    pub(crate) fn drain_server_requests_for_conversation(
        &mut self,
        conversation_id: &str,
        decision: ServerRequestDecision,
    ) -> Vec<(
        agent_protocol::RequestId,
        String,
        agent_protocol::ServerRequest,
        ServerRequestDecision,
    )> {
        self.server_requests
            .drain_conversation(conversation_id, decision)
    }

    pub(crate) fn pending_server_requests_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Vec<(agent_protocol::RequestId, agent_protocol::ServerRequest)> {
        self.server_requests.pending_for_conversation(conversation_id)
    }

    pub(crate) fn next_server_request_id(&self) -> agent_protocol::RequestId {
        self.server_requests.next_request_id()
    }

    pub(crate) fn insert_pending_server_request(
        &mut self,
        request_id: agent_protocol::RequestId,
        conversation_id: String,
        turn_id: String,
        request: agent_protocol::ServerRequest,
        reply_tx: oneshot::Sender<ServerRequestDecision>,
    ) {
        self.server_requests
            .insert_pending(request_id, conversation_id, turn_id, request, reply_tx);
    }
}

#[derive(Clone)]
pub(crate) struct TurnSpawnDependencies {
    pub(crate) event_tx: mpsc::UnboundedSender<AppServerMessage>,
    pub(crate) state: Arc<Mutex<ServerState>>,
    pub(crate) auto_approve: bool,
    pub(crate) auto_approve_reason: Option<String>,
}

pub(crate) async fn handle_command(
    runtime: Arc<AgentRuntime>,
    command: AppClientCommand,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<()> {
    match command {
        AppClientCommand::SubmitTurn(input) => {
            turn_service::submit_turn(
                runtime,
                event_tx,
                &state,
                input.conversation_id,
                input.content,
                auto_approve,
                auto_approve_reason,
            )
            .await;
        }
        AppClientCommand::InterruptTurn { conversation_id } => {
            turn_service::interrupt_turn(&runtime, event_tx, &state, conversation_id).await;
        }
        AppClientCommand::CompactConversation { conversation_id } => {
            turn_service::compact_conversation(&runtime, event_tx, &state, conversation_id).await?;
        }
        AppClientCommand::RequestConversationStatus { conversation_id } => {
            conversation_service::request_conversation_status(
                &runtime,
                event_tx,
                &state,
                conversation_id,
            )
            .await?;
        }
        AppClientCommand::RequestConversationHistory { conversation_id } => {
            conversation_service::request_conversation_history(
                &runtime,
                event_tx,
                &state,
                conversation_id.clone(),
            )
            .await?;
            server_request_service::replay_pending_for_conversation(
                event_tx,
                &state,
                &conversation_id,
            )
            .await;
        }
        AppClientCommand::ListConversations => {
            conversation_service::list_conversations(&runtime, event_tx, &state).await?;
        }
        AppClientCommand::CreateConversation { conversation_id } => {
            conversation_service::create_conversation(
                &runtime,
                event_tx,
                &state,
                conversation_id,
            )
            .await?;
        }
        AppClientCommand::SwitchConversation { conversation_id } => {
            conversation_service::switch_conversation(&runtime, event_tx, &state, conversation_id)
                .await?;
        }
        AppClientCommand::ArchiveConversation { conversation_id } => {
            conversation_service::archive_conversation(&runtime, event_tx, &state, conversation_id)
                .await?;
        }
        AppClientCommand::ResetConversation { conversation_id } => {
            conversation_service::reset_conversation(&runtime, event_tx, &state, conversation_id)
                .await?;
        }
        AppClientCommand::SubscribeConversation { conversation_id } => {
            conversation_service::subscribe_conversation(event_tx, &state, conversation_id.clone())
                .await;
            server_request_service::replay_pending_for_conversation(
                event_tx,
                &state,
                &conversation_id,
            )
            .await;
        }
        AppClientCommand::UnsubscribeConversation { conversation_id } => {
            conversation_service::unsubscribe_conversation(event_tx, &state, conversation_id).await;
        }
        AppClientCommand::ResolveServerRequest {
            conversation_id,
            request_id,
            decision,
        } => {
            server_request_service::resolve_command(
                &runtime,
                event_tx,
                &state,
                conversation_id,
                request_id,
                decision,
            )
            .await;
        }
        AppClientCommand::Exit => {}
    }

    Ok(())
}

pub(crate) fn merge_active_turn(turns: &mut Vec<ConversationTurn>, active_turn: Option<ConversationTurn>) {
    let Some(active_turn) = active_turn else {
        return;
    };
    if let Some(existing) = turns.iter_mut().find(|turn| turn.id == active_turn.id) {
        *existing = active_turn;
    } else {
        turns.push(active_turn);
    }
}

#[cfg(test)]
mod tests {
    use super::merge_active_turn;
    use agent_core::{ConversationTurn, TranscriptItem};
    use agent_protocol::TurnState;

    #[test]
    fn active_turn_snapshot_replaces_matching_rollout_turn() {
        let mut turns = vec![turn("turn-1", "old")];

        merge_active_turn(&mut turns, Some(turn("turn-1", "live")));

        assert_eq!(turns.len(), 1);
        assert!(matches!(
            &turns[0].items[..],
            [TranscriptItem::AgentMessage { text, .. }] if text == "live"
        ));
    }

    #[test]
    fn active_turn_snapshot_appends_when_rollout_has_no_matching_turn() {
        let mut turns = vec![turn("turn-1", "old")];

        merge_active_turn(&mut turns, Some(turn("turn-2", "live")));

        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].id, "turn-2");
    }

    fn turn(id: &str, text: &str) -> ConversationTurn {
        ConversationTurn {
            id: id.to_string(),
            state: TurnState::Running,
            items: vec![TranscriptItem::AgentMessage {
                id: format!("assistant:{id}"),
                text: text.to_string(),
            }],
            rollout_start_index: 0,
            rollout_end_index: 0,
        }
    }
}

