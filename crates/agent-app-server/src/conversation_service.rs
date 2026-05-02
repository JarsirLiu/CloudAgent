use crate::command_router::{ServerState, merge_active_turn};
use crate::notification_service::send_notification;
use agent_protocol::{AppServerMessage, AppServerNotification, ConversationStatus, FrontendMode};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

pub(crate) async fn list_conversations(
    runtime: &Arc<AgentRuntime>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
) -> Result<()> {
    let active_conversation_id = {
        let state = state.lock().await;
        state.active_conversation_id().to_string()
    };
    let conversations = runtime.list_conversations().await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationList {
            conversation_id: active_conversation_id,
            conversations,
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn request_conversation_history(
    runtime: &Arc<AgentRuntime>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    let active_listener = {
        let state = state.lock().await;
        state.active_listener(&conversation_id)
    };
    let active_turn = match active_listener {
        Some(listener) => listener.active_turn_snapshot().await,
        None => None,
    };
    let (mut turns, _has_more, _next_before_turn_id) = runtime
        .build_turns_page_from_rollout(&conversation_id, None, 30)
        .await?;
    merge_active_turn(&mut turns, active_turn);
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationHistory {
            conversation_id,
            turns,
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn request_conversation_status(
    runtime: &Arc<AgentRuntime>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    let snapshot = runtime.conversation_status(&conversation_id).await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationStatus {
            conversation_id,
            snapshot,
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn request_conversation_history_page(
    runtime: &Arc<AgentRuntime>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
    before_turn_id: Option<String>,
    limit: usize,
) -> Result<()> {
    let (turns, has_more, next_before_turn_id) = runtime
        .build_turns_page_from_rollout(&conversation_id, before_turn_id.as_deref(), limit)
        .await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationHistoryPage {
            conversation_id,
            turns,
            has_more,
            next_before_turn_id,
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn replay_frontend_state(
    runtime: &Arc<AgentRuntime>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: &str,
) -> Result<()> {
    let snapshot = runtime.conversation_status(conversation_id).await?;
    let mode = match snapshot.conversation_status {
        ConversationStatus::Busy => FrontendMode::Running,
        ConversationStatus::Idle => FrontendMode::Idle,
    };
    send_notification(
        event_tx,
        state,
        AppServerNotification::FrontendStateChanged {
            conversation_id: conversation_id.to_string(),
            mode,
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn create_conversation(
    runtime: &Arc<AgentRuntime>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    runtime.create_conversation(&conversation_id).await?;
    let conversations = runtime.list_conversations().await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationList {
            conversation_id: conversation_id.clone(),
            conversations,
        },
    )
    .await;
    send_notification(
        event_tx,
        state,
        AppServerNotification::Info {
            conversation_id,
            message: "conversation created".to_string(),
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn switch_conversation(
    runtime: &Arc<AgentRuntime>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    {
        let mut guard = state.lock().await;
        guard.switch_active_conversation(conversation_id.clone());
        guard.subscribe(conversation_id.clone());
    }
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationSwitched {
            conversation_id: conversation_id.clone(),
        },
    )
    .await;
    let snapshot = runtime.conversation_status(&conversation_id).await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationStatus {
            conversation_id: conversation_id.clone(),
            snapshot,
        },
    )
    .await;
    let turns = runtime.build_turns_from_rollout(&conversation_id).await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationHistory {
            conversation_id: conversation_id.clone(),
            turns,
        },
    )
    .await;
    let conversations = runtime.list_conversations().await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationList {
            conversation_id,
            conversations,
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn archive_conversation(
    runtime: &Arc<AgentRuntime>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    runtime.archive_conversation(&conversation_id).await?;
    let (active_conversation_id, switched_active) = {
        let mut guard = state.lock().await;
        let was_active = guard.active_conversation_id() == conversation_id;
        guard.unsubscribe(&conversation_id);
        if was_active {
            guard.switch_active_conversation(runtime.default_conversation_id().to_string());
            guard.subscribe(runtime.default_conversation_id().to_string());
        }
        (guard.active_conversation_id().to_string(), was_active)
    };
    if switched_active {
        send_notification(
            event_tx,
            state,
            AppServerNotification::ConversationSwitched {
                conversation_id: active_conversation_id.clone(),
            },
        )
        .await;
        let snapshot = runtime.conversation_status(&active_conversation_id).await?;
        send_notification(
            event_tx,
            state,
            AppServerNotification::ConversationStatus {
                conversation_id: active_conversation_id.clone(),
                snapshot,
            },
        )
        .await;
        let turns = runtime
            .build_turns_from_rollout(&active_conversation_id)
            .await?;
        send_notification(
            event_tx,
            state,
            AppServerNotification::ConversationHistory {
                conversation_id: active_conversation_id.clone(),
                turns,
            },
        )
        .await;
    }
    let conversations = runtime.list_conversations().await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationList {
            conversation_id: active_conversation_id.clone(),
            conversations,
        },
    )
    .await;
    if active_conversation_id != conversation_id {
        send_notification(
            event_tx,
            state,
            AppServerNotification::Info {
                conversation_id: active_conversation_id,
                message: format!("Archived conversation `{conversation_id}`"),
            },
        )
        .await;
    }
    Ok(())
}

pub(crate) async fn reset_conversation(
    runtime: &Arc<AgentRuntime>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    runtime.reset_conversation(&conversation_id).await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::Info {
            conversation_id,
            message: "conversation reset".to_string(),
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn subscribe_conversation(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) {
    {
        let mut state = state.lock().await;
        state.subscribe(conversation_id.clone());
    }
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationSubscriptionChanged {
            conversation_id,
            subscribed: true,
        },
    )
    .await;
}

pub(crate) async fn unsubscribe_conversation(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) {
    {
        let mut state = state.lock().await;
        state.unsubscribe(&conversation_id);
    }
    let _ = event_tx.send(AppServerMessage::Notification(
        AppServerNotification::ConversationSubscriptionChanged {
            conversation_id,
            subscribed: false,
        },
    ));
}
