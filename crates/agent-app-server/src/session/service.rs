use crate::routing::command_router::{ServerState, merge_active_turn};
use crate::app::notification::send_notification;
use crate::session::state as session_state;
use agent_protocol::{AppServerMessage, AppServerNotification, ConversationStatus, FrontendMode};
use agent_core::AgentHost;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio::task;

pub(crate) async fn list_conversations(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
) -> Result<()> {
    let active_session_id = {
        let state = state.lock().await;
        state.active_conversation_id().to_string()
    };
    let conversations = runtime.list_conversations().await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationList {
            conversation_id: active_session_id,
            conversations,
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn request_conversation_history(
    runtime: &Arc<AgentHost>,
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
    runtime: &Arc<AgentHost>,
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
    runtime: &Arc<AgentHost>,
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
    runtime: &Arc<AgentHost>,
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
    runtime: &Arc<AgentHost>,
    _event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    _state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    runtime.create_conversation(&conversation_id).await?;
    Ok(())
}

pub(crate) async fn switch_conversation(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    session_state::apply_active_conversation(state, conversation_id.clone()).await;
    session_state::persist_active_conversation(runtime, &conversation_id).await;
    publish_switched_conversation_state(runtime, event_tx, state, &conversation_id).await?;
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
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    runtime.archive_conversation(&conversation_id).await?;
    let next_conversation_id = runtime.create_fresh_conversation().await?;
    let transition = session_state::apply_archive_transition(
        state,
        &conversation_id,
        &next_conversation_id,
    )
    .await;
    let active_session_id = transition.active_session_id;
    let switched_active = transition.switched_active;
    if switched_active {
        session_state::persist_active_conversation(runtime, &active_session_id).await;
        publish_switched_conversation_state(runtime, event_tx, state, &active_session_id).await?;
    }
    let conversations = runtime.list_conversations().await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationList {
            conversation_id: active_session_id.clone(),
            conversations,
        },
    )
    .await;
    if active_session_id != conversation_id {
        send_notification(
            event_tx,
            state,
            AppServerNotification::Info {
                conversation_id: active_session_id,
                message: format!("Archived conversation `{conversation_id}`"),
            },
        )
        .await;
    }
    Ok(())
}

pub(crate) async fn reset_conversation(
    runtime: &Arc<AgentHost>,
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

pub(crate) async fn set_conversation_title(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
    title: String,
) -> Result<()> {
    runtime.set_conversation_title(&conversation_id, &title).await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::Info {
            conversation_id,
            message: "conversation title updated".to_string(),
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn delete_conversation(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    let was_active = {
        let guard = state.lock().await;
        guard.active_conversation_id() == conversation_id
    };
    runtime.reset_conversation(&conversation_id).await?;
    if was_active {
        let next_conversation_id = runtime.create_fresh_conversation().await?;
        session_state::apply_active_conversation(state, next_conversation_id.clone()).await;
        session_state::persist_active_conversation(runtime, &next_conversation_id).await;
        publish_switched_conversation_state(runtime, event_tx, state, &next_conversation_id).await?;
    }
    let active_session_id = {
        let guard = state.lock().await;
        guard.active_conversation_id().to_string()
    };
    let conversations = runtime.list_conversations().await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationList {
            conversation_id: active_session_id.clone(),
            conversations,
        },
    )
    .await;
    send_notification(
        event_tx,
        state,
        AppServerNotification::Info {
            conversation_id: active_session_id,
            message: format!("Deleted conversation `{conversation_id}`"),
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn maybe_spawn_auto_title_job(
    runtime: Arc<AgentHost>,
    event_tx: mpsc::UnboundedSender<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
    conversation_id: String,
    first_user_message: String,
) {
    if runtime.llm_model_name() == "fake-model" {
        return;
    }
    let already_titled = runtime
        .list_conversations()
        .await
        .ok()
        .and_then(|list| {
            list.into_iter()
                .find(|c| c.conversation_id == conversation_id)
                .and_then(|c| c.title)
        })
        .is_some();
    if already_titled {
        return;
    }
    {
        let mut guard = state.lock().await;
        if !guard.try_start_title_job(&conversation_id) {
            return;
        }
    }
    task::spawn(async move {
        let candidate = runtime
            .suggest_conversation_title(&first_user_message)
            .await
            .ok()
            .and_then(|t| normalize_title_candidate(&t))
            .unwrap_or_else(|| derive_title(&first_user_message));
        if !candidate.is_empty() {
            let still_untitled = runtime
                .list_conversations()
                .await
                .ok()
                .and_then(|list| {
                    list.into_iter()
                        .find(|c| c.conversation_id == conversation_id)
                        .and_then(|c| c.title)
                })
                .is_none();
            if still_untitled
                && runtime
                    .set_conversation_title(&conversation_id, &candidate)
                    .await
                    .is_ok()
            {
                let _ = list_conversations(&runtime, &event_tx, &state).await;
            }
        }
        state.lock().await.finish_title_job(&conversation_id);
    });
}

fn derive_title(input: &str) -> String {
    let single = input.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_with_ellipsis(single.trim(), 40)
}

fn normalize_title_candidate(raw: &str) -> Option<String> {
    let cleaned = raw
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim();
    if cleaned.is_empty() {
        return None;
    }
    let mut collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    // Reject obviously rambling paragraphs; fallback will use user message snippet.
    if collapsed.chars().count() > 72 {
        return None;
    }
    collapsed = truncate_with_ellipsis(&collapsed, 40);
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

fn truncate_with_ellipsis(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max_chars.saturating_sub(1) {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
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

async fn publish_switched_conversation_state(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: &str,
) -> Result<()> {
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationSwitched {
            conversation_id: conversation_id.to_string(),
        },
    )
    .await;
    let snapshot = runtime.conversation_status(conversation_id).await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationStatus {
            conversation_id: conversation_id.to_string(),
            snapshot,
        },
    )
    .await;
    let turns = runtime.build_turns_from_rollout(conversation_id).await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationHistory {
            conversation_id: conversation_id.to_string(),
            turns,
        },
    )
    .await;
    Ok(())
}


