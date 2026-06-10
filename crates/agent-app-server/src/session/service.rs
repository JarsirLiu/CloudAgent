use crate::app::notification::send_notification;
use crate::routing::command_router::{ServerState, merge_active_turn};
use crate::session::conversation_watch::ConversationWatchManager;
use crate::session::state as session_state;
use agent_core::{
    AgentHost, ConversationTurn, InputItem, TranscriptItem, TurnState, input_items_preview_text,
};
use agent_protocol::{
    AppServerMessage, AppServerNotification, ConversationHistoryPageResponse,
    ConversationHistoryResponse, ConversationListPageResponse, ConversationViewResponse,
    ConversationViewSnapshot, SkillsListResponse,
};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio::task;

pub(crate) async fn list_conversations_page(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    cursor: Option<String>,
    limit: usize,
) -> Result<()> {
    let anchor_conversation_id = {
        let state = state.lock().await;
        state
            .notification_anchor_conversation_id("default")
            .to_string()
    };
    let page = read_conversation_list_page(runtime, state, cursor, limit).await?;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ConversationListPage {
            conversation_id: anchor_conversation_id,
            conversations: page.conversations,
            has_more: page.has_more,
            next_cursor: page.next_cursor,
        },
    )
    .await;
    Ok(())
}

pub(crate) async fn read_conversation_list_page(
    runtime: &Arc<AgentHost>,
    _state: &Arc<Mutex<ServerState>>,
    cursor: Option<String>,
    limit: usize,
) -> Result<ConversationListPageResponse> {
    reconcile_missing_for_list(runtime).await;
    let page = runtime.list_conversations_page(cursor, limit).await?;
    Ok(ConversationListPageResponse {
        conversations: page.conversations,
        has_more: page.has_more,
        next_cursor: page.next_cursor,
    })
}

pub(crate) async fn read_skills_list(
    runtime: &Arc<AgentHost>,
    _state: &Arc<Mutex<ServerState>>,
) -> Result<SkillsListResponse> {
    Ok(SkillsListResponse {
        skills: runtime.list_skills(),
    })
}

pub(crate) async fn notify_skills_changed(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
) {
    let conversation_id = {
        let guard = state.lock().await;
        guard
            .notification_anchor_conversation_id("default")
            .to_string()
    };
    send_notification(
        event_tx,
        state,
        AppServerNotification::SkillsChanged { conversation_id },
    )
    .await;
}

pub(crate) async fn request_conversation_history(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    if runtime
        .purge_missing_conversation_if_needed(&conversation_id)
        .await?
    {
        send_notification(
            event_tx,
            state,
            AppServerNotification::Error {
                conversation_id,
                message: "Conversation data is missing; removed stale session metadata."
                    .to_string(),
            },
        )
        .await;
        return Ok(());
    }
    let active_listener = {
        let state = state.lock().await;
        state.active_listener(&conversation_id)
    };
    let active_turn = match active_listener {
        Some(listener) => listener.active_turn_snapshot().await,
        None => None,
    };
    let mut turns = runtime.build_turns_from_rollout(&conversation_id).await?;
    recover_inactive_running_turns(&mut turns, active_turn.as_ref());
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

pub(crate) async fn read_conversation_history(
    runtime: &Arc<AgentHost>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<ConversationHistoryResponse> {
    if runtime
        .purge_missing_conversation_if_needed(&conversation_id)
        .await?
    {
        anyhow::bail!("conversation data is missing; removed stale session metadata");
    }
    let active_listener = {
        let state = state.lock().await;
        state.active_listener(&conversation_id)
    };
    let active_turn = match active_listener {
        Some(listener) => listener.active_turn_snapshot().await,
        None => None,
    };
    let mut turns = runtime.build_turns_from_rollout(&conversation_id).await?;
    recover_inactive_running_turns(&mut turns, active_turn.as_ref());
    merge_active_turn(&mut turns, active_turn);
    Ok(ConversationHistoryResponse { turns })
}

pub(crate) async fn request_conversation_view(
    runtime: &Arc<AgentHost>,
    view: &ConversationWatchManager,
    conversation_id: String,
) -> Result<()> {
    hydrate_conversation_view(runtime, view, &conversation_id).await?;
    view.emit_current(&conversation_id).await;
    Ok(())
}

pub(crate) async fn read_conversation_view(
    runtime: &Arc<AgentHost>,
    view: &ConversationWatchManager,
    conversation_id: String,
) -> Result<ConversationViewResponse> {
    Ok(ConversationViewResponse {
        snapshot: conversation_view_snapshot(runtime, view, &conversation_id).await?,
    })
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
    let mut turns = turns;
    let active_turn = active_turn_snapshot(state, &conversation_id).await;
    recover_inactive_running_turns(&mut turns, active_turn.as_ref());
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

pub(crate) async fn read_conversation_history_page(
    runtime: &Arc<AgentHost>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
    before_turn_id: Option<String>,
    limit: usize,
) -> Result<ConversationHistoryPageResponse> {
    let (turns, has_more, next_before_turn_id) = runtime
        .build_turns_page_from_rollout(&conversation_id, before_turn_id.as_deref(), limit)
        .await?;
    let mut turns = turns;
    let active_turn = active_turn_snapshot(state, &conversation_id).await;
    recover_inactive_running_turns(&mut turns, active_turn.as_ref());
    Ok(ConversationHistoryPageResponse {
        turns,
        has_more,
        next_before_turn_id,
    })
}

pub(crate) async fn replay_conversation_view_state(
    runtime: &Arc<AgentHost>,
    view: &ConversationWatchManager,
    conversation_id: &str,
) -> Result<()> {
    request_conversation_view(runtime, view, conversation_id.to_string()).await
}

async fn active_turn_snapshot(
    state: &Arc<Mutex<ServerState>>,
    conversation_id: &str,
) -> Option<ConversationTurn> {
    let active_listener = {
        let state = state.lock().await;
        state.active_listener(conversation_id)
    };
    match active_listener {
        Some(listener) => listener.active_turn_snapshot().await,
        None => None,
    }
}

async fn conversation_view_snapshot(
    runtime: &Arc<AgentHost>,
    view: &ConversationWatchManager,
    conversation_id: &str,
) -> Result<ConversationViewSnapshot> {
    hydrate_conversation_view(runtime, view, conversation_id).await?;
    Ok(view.snapshot(conversation_id).await)
}

async fn hydrate_conversation_view(
    runtime: &Arc<AgentHost>,
    view: &ConversationWatchManager,
    conversation_id: &str,
) -> Result<()> {
    let message_count = runtime
        .conversation_status(conversation_id)
        .await
        .map(|status| status.message_count)
        .unwrap_or(0);
    view.note_loaded(conversation_id, message_count).await;
    Ok(())
}

fn recover_inactive_running_turns(
    turns: &mut [ConversationTurn],
    active_turn: Option<&ConversationTurn>,
) {
    let active_turn_id = active_turn.map(|turn| turn.id.as_str());
    for turn in turns {
        if turn.state != TurnState::Running || active_turn_id == Some(turn.id.as_str()) {
            continue;
        }
        turn.state = TurnState::Cancelled;
        let notice_id = format!("turn_interrupted:{}", turn.id);
        if turn.items.iter().any(|item| item.id() == notice_id) {
            continue;
        }
        turn.items.push(TranscriptItem::SystemMessage {
            id: notice_id,
            text: "Previous process exited before this turn completed.".to_string(),
        });
    }
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
    view: &ConversationWatchManager,
    conversation_id: String,
) -> Result<()> {
    if runtime
        .purge_missing_conversation_if_needed(&conversation_id)
        .await?
    {
        send_notification(
            event_tx,
            state,
            AppServerNotification::Error {
                conversation_id,
                message: "Conversation data is missing; removed stale session metadata."
                    .to_string(),
            },
        )
        .await;
        return Ok(());
    }
    session_state::apply_active_conversation(state, conversation_id.clone()).await;
    session_state::persist_active_conversation(runtime, state, &conversation_id).await;
    publish_switched_conversation_state(runtime, event_tx, state, view, &conversation_id).await?;
    Ok(())
}

pub(crate) async fn archive_conversation(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    view: &ConversationWatchManager,
    conversation_id: String,
) -> Result<()> {
    runtime.archive_conversation(&conversation_id).await?;
    let next_conversation_id = runtime.create_conversation_with_timestamp_id().await?;
    let transition =
        session_state::apply_archive_transition(state, &conversation_id, &next_conversation_id)
            .await;
    let active_session_id = transition.active_session_id;
    let switched_active = transition.switched_active;
    if switched_active {
        session_state::persist_active_conversation(runtime, state, &active_session_id).await;
        publish_switched_conversation_state(runtime, event_tx, state, view, &active_session_id)
            .await?;
    }
    if active_session_id != conversation_id && !switched_active {
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
    runtime
        .set_conversation_title(&conversation_id, &title)
        .await?;
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
    view: &ConversationWatchManager,
    conversation_id: String,
) -> Result<()> {
    let was_active = {
        let guard = state.lock().await;
        guard.tracks_active_conversation() && guard.active_conversation_id() == conversation_id
    };
    if was_active {
        runtime.reset_conversation(&conversation_id).await?;
    } else {
        runtime.delete_conversation(&conversation_id).await?;
    }
    if was_active {
        let next_conversation_id = runtime.create_conversation_with_timestamp_id().await?;
        session_state::apply_active_conversation(state, next_conversation_id.clone()).await;
        session_state::persist_active_conversation(runtime, state, &next_conversation_id).await;
        publish_switched_conversation_state(runtime, event_tx, state, view, &next_conversation_id)
            .await?;
    }
    let active_session_id = {
        let guard = state.lock().await;
        guard
            .notification_anchor_conversation_id(&conversation_id)
            .to_string()
    };
    if !was_active {
        send_notification(
            event_tx,
            state,
            AppServerNotification::Info {
                conversation_id: active_session_id,
                message: format!("Deleted conversation `{conversation_id}`"),
            },
        )
        .await;
    }
    Ok(())
}

async fn reconcile_missing_for_list(runtime: &Arc<AgentHost>) {
    if let Err(err) = runtime.reconcile_missing_conversations(100).await {
        tracing::debug!("failed to reconcile missing conversations: {err:#}");
    }
}

pub(crate) async fn report_hub_mode_only_command(
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
    send_notification(
        event_tx,
        state,
        AppServerNotification::Error {
            conversation_id,
            message: format!(
                "hub mode only: `{command_name}` is not available for the current direct target"
            ),
        },
    )
    .await;
}

pub(crate) async fn maybe_spawn_auto_title_job(
    runtime: Arc<AgentHost>,
    event_tx: mpsc::UnboundedSender<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
    conversation_id: String,
    first_user_message: Vec<InputItem>,
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
                let _ = list_conversations_page(&runtime, &event_tx, &state, None, 25).await;
            }
        }
        state.lock().await.finish_title_job(&conversation_id);
    });
}

fn derive_title(input: &[InputItem]) -> String {
    truncate_with_ellipsis(&input_items_preview_text(input, 40), 40)
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
    view: &ConversationWatchManager,
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
    hydrate_conversation_view(runtime, view, conversation_id).await?;
    view.emit_current(conversation_id).await;
    let mut turns = runtime.build_turns_from_rollout(conversation_id).await?;
    let active_turn = active_turn_snapshot(state, conversation_id).await;
    recover_inactive_running_turns(&mut turns, active_turn.as_ref());
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

#[cfg(test)]
#[path = "session_service_tests.rs"]
mod session_service_tests;
