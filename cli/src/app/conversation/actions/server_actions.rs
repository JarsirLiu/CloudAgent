use super::show_local_notice;
use crate::app::TuiApp;
use crate::app::conversation::facade as conversation_facade;
use crate::state::NoticeLevel;
use crate::state::reducer::ServerAction;
use agent_app_server_client::AppServerClient;
use agent_protocol::InterruptDisposition;
use anyhow::Result;
use std::collections::HashSet;
use tokio::time::{Duration, timeout};

const HISTORY_PAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) async fn load_older_history_page_if_available(
    app: &mut TuiApp,
    client: &AppServerClient,
) -> Result<bool> {
    if app.current_mode() != agent_protocol::FrontendMode::Idle || !app.run_state.history_has_more {
        return Ok(false);
    }
    if !app.bottom_pane.composer_is_empty() {
        return Ok(false);
    }
    let Some(limit) = app.conversation_history_turn_limit else {
        return Ok(false);
    };

    let Some(before_turn_id) = app.run_state.history_next_before_turn_id.clone() else {
        return Ok(false);
    };

    app.run_state.history_has_more = false;
    let response = match timeout(
        HISTORY_PAGE_REQUEST_TIMEOUT,
        client.request_conversation_history_page_typed(
            &app.conversation_id,
            Some(before_turn_id),
            limit,
        ),
    )
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => {
            app.run_state.history_has_more = true;
            show_local_notice(
                app,
                NoticeLevel::Warn,
                format!("Failed to load older history: {err}"),
            );
            return Ok(false);
        }
        Err(_) => {
            app.run_state.history_has_more = true;
            show_local_notice(app, NoticeLevel::Warn, "Timed out loading older history");
            return Ok(false);
        }
    };

    execute_server_action(
        app,
        ServerAction::PrependHistoryPage {
            turns: response.turns,
            has_more: response.has_more,
            next_before_turn_id: response.next_before_turn_id,
        },
    );
    Ok(true)
}

pub(crate) fn execute_server_action(app: &mut TuiApp, action: ServerAction) {
    match action {
        ServerAction::SetConversationListPage {
            conversations,
            has_more,
            next_cursor,
        } => {
            if app.bottom_pane.present_requested_session_picker_page(
                conversations.clone(),
                &app.conversation_id,
                has_more,
                next_cursor.clone(),
            ) {
                app.conversation_summaries = conversations;
            } else if app.bottom_pane.append_session_page(
                conversations.clone(),
                has_more,
                next_cursor,
            ) {
                for conversation in conversations.clone() {
                    if app
                        .conversation_summaries
                        .iter()
                        .any(|existing| existing.conversation_id == conversation.conversation_id)
                    {
                        continue;
                    }
                    app.conversation_summaries.push(conversation);
                }
            }
        }
        ServerAction::InvalidateSkillsCatalog => {
            app.run_state.pending_skills_refresh = true;
            app.run_state.next_skills_refresh_at = None;
        }
        ServerAction::SetConversationView(snapshot) => {
            app.apply_conversation_view_snapshot(snapshot);
        }
        ServerAction::SwitchConversation(conversation_id) => {
            app.bottom_pane.clear_session_picker();
            app.switch_conversation(conversation_id);
        }
        ServerAction::ClearCurrentTurnUsage => {
            app.on_server_turn_started();
        }
        ServerAction::SetTokenUsage {
            last_usage,
            total_usage,
            model_context_window,
        } => {
            app.run_state.last_turn_usage = Some(last_usage);
            app.run_state.total_turn_usage = Some(total_usage);
            app.run_state.model_context_window = model_context_window;
        }
        ServerAction::SetRetryStatus {
            stage,
            attempt,
            next_delay_ms,
        } => {
            app.on_server_retrying(stage, attempt, next_delay_ms);
        }
        ServerAction::SetContextCompactionStatus { estimated_tokens } => {
            app.bottom_pane
                .on_context_compaction_started(estimated_tokens);
        }
        ServerAction::ClearContextCompactionStatus => {
            app.bottom_pane.on_context_compaction_finished();
        }
        ServerAction::DismissServerRequestView(request_id) => {
            app.dismiss_server_request_view(&request_id);
        }
        ServerAction::ReplaceHistory(messages) => {
            app.run_state.history_snapshot = Some(messages);
            app.run_state.history_has_more = false;
            app.run_state.history_next_before_turn_id = None;
            conversation_facade::replace_transcript_from_history(app);
        }
        ServerAction::ReplaceHistoryPage {
            turns,
            has_more,
            next_before_turn_id,
        } => {
            app.run_state.history_snapshot = Some(turns);
            app.run_state.history_has_more = has_more;
            app.run_state.history_next_before_turn_id = next_before_turn_id;
            conversation_facade::replace_transcript_from_history(app);
        }
        ServerAction::PrependHistoryPage {
            turns,
            has_more,
            next_before_turn_id,
        } => {
            let existing = app.run_state.history_snapshot.take().unwrap_or_default();
            app.run_state.history_snapshot = Some(prepend_turn_page(turns, existing));
            app.run_state.history_has_more = has_more;
            app.run_state.history_next_before_turn_id = next_before_turn_id;
            conversation_facade::rebuild_transcript_from_history(app);
        }
        ServerAction::UpsertTurnSnapshot(turn) => {
            conversation_facade::upsert_turn_snapshot(app, turn);
        }
        ServerAction::BindActiveTurn(turn_id) => {
            app.transcript_owner
                .bind_turn_id(turn_id, app.run_state.expand_tool_details);
        }
        ServerAction::StartActiveTurnItem { turn_id, item } => {
            app.on_server_active_item_started(&item);
            app.transcript_owner
                .start_item(turn_id, item, app.run_state.expand_tool_details);
        }
        ServerAction::AppendActiveAgentDelta {
            turn_id,
            item_id,
            delta,
        } => {
            app.transcript_owner.append_agent_delta(
                turn_id,
                item_id,
                delta,
                app.run_state.expand_tool_details,
            );
        }
        ServerAction::AppendActiveReasoningDelta {
            turn_id,
            item_id,
            delta,
        } => {
            app.transcript_owner.append_reasoning_delta(
                turn_id,
                item_id,
                delta,
                app.run_state.expand_tool_details,
            );
        }
        ServerAction::AppendActiveRuntimeDelta {
            turn_id,
            item_id,
            delta,
        } => {
            app.bottom_pane
                .on_active_runtime_output_delta(Some(&item_id), &delta);
            app.transcript_owner.append_tool_delta(
                turn_id,
                item_id,
                delta,
                app.run_state.expand_tool_details,
            );
        }
        ServerAction::AppendActivePatchDelta {
            turn_id,
            item_id,
            delta,
        } => {
            app.transcript_owner.append_patch_delta(
                turn_id,
                item_id,
                delta,
                app.run_state.expand_tool_details,
            );
        }
        ServerAction::UpdateActiveItemProgress {
            turn_id,
            item_id,
            progress,
        } => {
            app.bottom_pane.on_item_progress(Some(&item_id), &progress);
            app.transcript_owner.update_item_progress(
                turn_id,
                item_id,
                progress,
                app.run_state.expand_tool_details,
            );
        }
        ServerAction::UpdateActiveItemMetrics {
            turn_id,
            item_id,
            metrics,
        } => {
            app.bottom_pane
                .on_item_metrics_updated(Some(&item_id), &metrics);
            app.transcript_owner.update_item_metrics(
                turn_id,
                item_id,
                metrics,
                app.run_state.expand_tool_details,
            );
        }
        ServerAction::AppendActiveRuntimeOutputDelta { item_id, delta } => {
            app.bottom_pane
                .on_active_runtime_output_delta(Some(&item_id), &delta);
        }
        ServerAction::CompleteActiveTurnItem {
            turn_id,
            item,
            transcript_item,
        } => {
            app.bottom_pane
                .on_active_item_completed(&item, &transcript_item);
            app.transcript_owner.complete_item(
                turn_id,
                item.id,
                transcript_item,
                app.run_state.expand_tool_details,
            );
        }
        ServerAction::PushNoticeCell {
            label,
            message,
            level,
        } => {
            if matches!(level, NoticeLevel::Error) {
                app.bottom_pane.clear_views();
            }
            if app.should_suppress_notice(&label, &message) {
                return;
            }
            app.bottom_pane.push_toast(level, message);
        }
        ServerAction::InterruptResult(disposition) => match disposition {
            InterruptDisposition::Requested => {
                app.bottom_pane
                    .push_toast(NoticeLevel::Info, "interrupt requested".to_string());
            }
            InterruptDisposition::NoActiveTurn => {
                app.run_state.turn_lifecycle.clear_pending_submission();
                app.bottom_pane.on_turn_finished();
                app.clear_server_request_view();
                app.transcript_owner
                    .clear_active_turn(app.run_state.expand_tool_details);
                app.transcript_scroll.reset();
                app.bottom_pane
                    .push_toast(NoticeLevel::Warn, "no active turn".to_string());
            }
        },
        ServerAction::TurnDispatch(dispatch) => {
            conversation_facade::apply_turn_dispatch(app, dispatch);
        }
        ServerAction::ShowServerRequestPrompt {
            request_id,
            request,
        } => {
            app.show_server_request_prompt(
                crate::ui::bottom_pane::input_pane::ServerRequestInlineState {
                    request_id,
                    presentation: request.clone(),
                },
            );
            app.bottom_pane
                .push_toast(NoticeLevel::Warn, request.notice_text());
        }
    }
}

pub(crate) fn prepend_turn_page(
    older_turns: Vec<agent_core::ConversationTurn>,
    existing_turns: Vec<agent_core::ConversationTurn>,
) -> Vec<agent_core::ConversationTurn> {
    let mut seen = HashSet::new();
    let mut merged = Vec::with_capacity(older_turns.len() + existing_turns.len());
    for turn in older_turns.into_iter().chain(existing_turns) {
        if seen.insert(turn.id.clone()) {
            merged.push(turn);
        }
    }
    merged
}
