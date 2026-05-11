use crate::server_request::coordinator::{ResolvedServerRequest, ServerRequestCoordinator};
use crate::server_request::service as server_request_service;
use crate::session::listener::ConversationListenerHandle;
use crate::session::service as session_service;
use crate::session::subscriptions::ConversationSubscriptions;
use crate::turn::service as turn_service;
use agent_core::AgentHost;
use agent_core::ConversationTurn;
use agent_core::{ServerRequest, ServerRequestDecision, TurnState};
use agent_protocol::{AppClientCommand, AppServerMessage};
use anyhow::Result;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;

pub(crate) struct ServerState {
    active_conversation_id: String,
    emit_all_conversations: bool,
    subscriptions: ConversationSubscriptions,
    server_requests: ServerRequestCoordinator,
    turn_tasks_by_conversation: HashMap<String, Vec<JoinHandle<()>>>,
    active_listeners: HashMap<String, ConversationListenerHandle>,
    title_jobs_in_flight: HashSet<String>,
}

impl ServerState {
    pub(crate) fn new(default_conversation_id: String, emit_all_conversations: bool) -> Self {
        Self {
            active_conversation_id: default_conversation_id.clone(),
            emit_all_conversations,
            subscriptions: ConversationSubscriptions::new(default_conversation_id),
            server_requests: ServerRequestCoordinator::new(),
            turn_tasks_by_conversation: HashMap::new(),
            active_listeners: HashMap::new(),
            title_jobs_in_flight: HashSet::new(),
        }
    }

    pub(crate) fn track_turn_task(&mut self, conversation_id: String, task: JoinHandle<()>) {
        let tasks = self
            .turn_tasks_by_conversation
            .entry(conversation_id)
            .or_default();
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
    }

    pub(crate) fn take_turn_tasks_for_conversation(
        &mut self,
        conversation_id: &str,
    ) -> Vec<JoinHandle<()>> {
        self.turn_tasks_by_conversation
            .remove(conversation_id)
            .unwrap_or_default()
    }

    pub(crate) fn take_all_turn_tasks(&mut self) -> Vec<JoinHandle<()>> {
        self.turn_tasks_by_conversation
            .drain()
            .flat_map(|(_, tasks)| tasks)
            .collect()
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

    pub(crate) fn tracks_active_conversation(&self) -> bool {
        !self.emit_all_conversations
    }

    pub(crate) fn switch_active_conversation(&mut self, conversation_id: String) {
        if self.tracks_active_conversation() {
            self.active_conversation_id = conversation_id;
        }
    }

    pub(crate) fn notification_anchor_conversation_id<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.tracks_active_conversation() {
            self.active_conversation_id()
        } else {
            fallback
        }
    }

    pub(crate) fn is_subscribed(&self, conversation_id: &str) -> bool {
        self.emit_all_conversations || self.subscriptions.is_subscribed(conversation_id)
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
    ) -> Option<ResolvedServerRequest> {
        self.server_requests.resolve(request_id, decision)
    }

    pub(crate) fn drain_server_requests_for_turn(
        &mut self,
        turn_id: &str,
        decision: ServerRequestDecision,
    ) -> Vec<(
        agent_protocol::RequestId,
        String,
        ServerRequest,
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
        ServerRequest,
        ServerRequestDecision,
    )> {
        self.server_requests
            .drain_conversation(conversation_id, decision)
    }

    pub(crate) fn pending_server_requests_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Vec<(agent_protocol::RequestId, ServerRequest)> {
        self.server_requests
            .pending_for_conversation(conversation_id)
    }

    pub(crate) fn next_server_request_id(&self) -> agent_protocol::RequestId {
        self.server_requests.next_request_id()
    }

    pub(crate) fn insert_pending_server_request(
        &mut self,
        request_id: agent_protocol::RequestId,
        conversation_id: String,
        turn_id: String,
        request: ServerRequest,
        reply_tx: oneshot::Sender<ServerRequestDecision>,
    ) {
        self.server_requests.insert_pending(
            request_id,
            conversation_id,
            turn_id,
            request,
            reply_tx,
        );
    }

    pub(crate) fn try_start_title_job(&mut self, conversation_id: &str) -> bool {
        self.title_jobs_in_flight
            .insert(conversation_id.to_string())
    }

    pub(crate) fn finish_title_job(&mut self, conversation_id: &str) {
        self.title_jobs_in_flight.remove(conversation_id);
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
    runtime: Arc<AgentHost>,
    command: AppClientCommand,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<()> {
    match command {
        AppClientCommand::SubmitTurn(input) => {
            {
                let mut guard = state.lock().await;
                guard.subscribe(input.conversation_id.clone());
            }
            turn_service::submit_turn(
                runtime,
                event_tx,
                &state,
                input.conversation_id,
                input.content,
                input.turn_policy.permission_profile,
                input.turn_policy.approval_policy,
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
            session_service::request_conversation_status(
                &runtime,
                event_tx,
                &state,
                conversation_id,
            )
            .await?;
        }
        AppClientCommand::RequestConversationHistory { conversation_id } => {
            session_service::request_conversation_history(
                &runtime,
                event_tx,
                &state,
                conversation_id.clone(),
            )
            .await?;
            session_service::replay_frontend_state(&runtime, event_tx, &state, &conversation_id)
                .await?;
            server_request_service::replay_pending_for_conversation(
                event_tx,
                &state,
                &conversation_id,
            )
            .await;
        }
        AppClientCommand::RequestConversationHistoryPage {
            conversation_id,
            before_turn_id,
            limit,
        } => {
            session_service::request_conversation_history_page(
                &runtime,
                event_tx,
                &state,
                conversation_id,
                before_turn_id,
                limit,
            )
            .await?;
        }
        AppClientCommand::ListConversations => {
            session_service::list_conversations(&runtime, event_tx, &state).await?;
        }
        AppClientCommand::ListOnlineNodes => {
            session_service::report_hub_mode_only_command(event_tx, &state, "ListOnlineNodes")
                .await;
        }
        AppClientCommand::ListPlatforms => {
            report_node_managed_only_command(event_tx, &state, "ListPlatforms").await;
        }
        AppClientCommand::GetNodeStatus => {
            report_node_managed_only_command(event_tx, &state, "GetNodeStatus").await;
        }
        AppClientCommand::StopNode => {
            report_node_managed_only_command(event_tx, &state, "StopNode").await;
        }
        AppClientCommand::CreateConversation { conversation_id } => {
            session_service::create_conversation(&runtime, event_tx, &state, conversation_id)
                .await?;
        }
        AppClientCommand::SetConversationTitle {
            conversation_id,
            title,
        } => {
            session_service::set_conversation_title(
                &runtime,
                event_tx,
                &state,
                conversation_id,
                title,
            )
            .await?;
        }
        AppClientCommand::SwitchConversation { conversation_id } => {
            session_service::switch_conversation(&runtime, event_tx, &state, conversation_id)
                .await?;
        }
        AppClientCommand::SelectTargetNode { .. } => {
            session_service::report_hub_mode_only_command(event_tx, &state, "SelectTargetNode")
                .await;
        }
        AppClientCommand::GetPlatformStatus { .. } => {
            report_node_managed_only_command(event_tx, &state, "GetPlatformStatus").await;
        }
        AppClientCommand::GetPlatformConfig { .. } => {
            report_node_managed_only_command(event_tx, &state, "GetPlatformConfig").await;
        }
        AppClientCommand::SetPlatformEnabled { .. } => {
            report_node_managed_only_command(event_tx, &state, "SetPlatformEnabled").await;
        }
        AppClientCommand::SetPlatformConfigValue { .. } => {
            report_node_managed_only_command(event_tx, &state, "SetPlatformConfigValue").await;
        }
        AppClientCommand::ClearPlatformConfigValue { .. } => {
            report_node_managed_only_command(event_tx, &state, "ClearPlatformConfigValue").await;
        }
        AppClientCommand::ArchiveConversation { conversation_id } => {
            session_service::archive_conversation(&runtime, event_tx, &state, conversation_id)
                .await?;
        }
        AppClientCommand::DeleteConversation { conversation_id } => {
            session_service::delete_conversation(&runtime, event_tx, &state, conversation_id)
                .await?;
        }
        AppClientCommand::ResetConversation { conversation_id } => {
            session_service::reset_conversation(&runtime, event_tx, &state, conversation_id)
                .await?;
        }
        AppClientCommand::SubscribeConversation { conversation_id } => {
            session_service::subscribe_conversation(event_tx, &state, conversation_id.clone())
                .await;
            session_service::replay_frontend_state(&runtime, event_tx, &state, &conversation_id)
                .await?;
            server_request_service::replay_pending_for_conversation(
                event_tx,
                &state,
                &conversation_id,
            )
            .await;
        }
        AppClientCommand::UnsubscribeConversation { conversation_id } => {
            session_service::unsubscribe_conversation(event_tx, &state, conversation_id).await;
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

async fn report_node_managed_only_command(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    command_name: &str,
) {
    let conversation_id = {
        let guard = state.lock().await;
        guard
            .notification_anchor_conversation_id("default")
            .to_string()
    };
    let _ = event_tx.send(AppServerMessage::Notification(
        agent_protocol::AppServerNotification::Error {
            conversation_id,
            message: format!("node-managed only: `{command_name}` is not available for embedded app-server targets"),
        },
    ));
}

pub(crate) fn merge_active_turn(
    turns: &mut Vec<ConversationTurn>,
    active_turn: Option<ConversationTurn>,
) {
    let Some(active_turn) = active_turn else {
        return;
    };
    if let Some(existing) = turns.iter_mut().find(|turn| turn.id == active_turn.id) {
        let mut merged_items = existing.items.clone();
        for active_item in active_turn.items {
            if let Some(index) = merged_items
                .iter()
                .position(|existing_item| existing_item.id() == active_item.id())
            {
                merged_items[index] = active_item;
            } else {
                merged_items.push(active_item);
            }
        }
        existing.items = merged_items;
        existing.rollout_start_index = existing
            .rollout_start_index
            .min(active_turn.rollout_start_index);
        existing.rollout_end_index = existing
            .rollout_end_index
            .max(active_turn.rollout_end_index);
        if !matches!(
            existing.state,
            TurnState::Completed | TurnState::Failed | TurnState::Cancelled
        ) {
            existing.state = active_turn.state;
        }
    } else {
        turns.push(active_turn);
    }
}

#[cfg(test)]
mod tests {
    use super::merge_active_turn;
    use crate::routing::command_router::ServerState;
    use agent_core::{ConversationTurn, TranscriptItem, TurnState};

    #[test]
    fn active_turn_snapshot_merges_matching_rollout_turn_without_dropping_existing_items() {
        let mut turns = vec![ConversationTurn {
            id: "turn-1".to_string(),
            state: TurnState::Completed,
            items: vec![
                TranscriptItem::Reasoning {
                    id: "reasoning:1".to_string(),
                    title: "Reasoning".to_string(),
                    text: "thinking".to_string(),
                },
                TranscriptItem::AgentMessage {
                    id: "assistant:1".to_string(),
                    text: "final answer".to_string(),
                },
            ],
            rollout_start_index: 1,
            rollout_end_index: 4,
        }];

        merge_active_turn(
            &mut turns,
            Some(ConversationTurn {
                id: "turn-1".to_string(),
                state: TurnState::Running,
                items: vec![TranscriptItem::Reasoning {
                    id: "reasoning:1".to_string(),
                    title: "Reasoning".to_string(),
                    text: "thinking".to_string(),
                }],
                rollout_start_index: 1,
                rollout_end_index: 3,
            }),
        );

        assert_eq!(turns.len(), 1);
        assert!(matches!(
            &turns[0].items[..],
            [
                TranscriptItem::Reasoning { .. },
                TranscriptItem::AgentMessage { text, .. }
            ] if text == "final answer"
        ));
        assert!(matches!(turns[0].state, TurnState::Completed));
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

    #[test]
    fn shared_worker_state_uses_fallback_notification_anchor() {
        let mut state = ServerState::new("default".to_string(), true);
        state.switch_active_conversation("conversation-1".to_string());

        assert!(!state.tracks_active_conversation());
        assert_eq!(state.active_conversation_id(), "default");
        assert_eq!(
            state.notification_anchor_conversation_id("conversation-1"),
            "conversation-1"
        );
    }
}
