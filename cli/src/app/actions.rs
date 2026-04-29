use crate::app::effects::copy_text_to_clipboard;
use crate::app::parse::ParsedInput;
use crate::app::TuiApp;
use crate::state::reducer::{ItemDispatch, ServerAction};
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use agent_app_server_client::AppServerClient;
use agent_protocol::{AppClientCommand, FrontendMode, UserTurnInput};
use anyhow::Result;

pub(crate) fn handle_tui_input(
    session_id: &str,
    app: &mut TuiApp,
    client: &AppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::LocalCopy => {
            let Some(text) = app.transcript_state.last_copyable_output.as_deref() else {
                app.push_cell(HistoryCell::from_message(
                    "session",
                    "`/copy` unavailable before first assistant output",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            };
            match copy_text_to_clipboard(text) {
                Ok(()) => {
                    app.run_state.status_notice = Some("Copied latest assistant output".to_string());
                }
                Err(err) => {
                    app.push_cell(HistoryCell::from_message(
                        "error",
                        format!("failed to copy: {err}"),
                        HistoryTone::Error,
                    ));
                }
            }
        }
        ParsedInput::Command(command) => {
            if let AppClientCommand::Exit = command {
                if app.console_state.mode != FrontendMode::Idle {
                    client.send_command(AppClientCommand::InterruptTurn {
                        session_id: session_id.to_string(),
                    })?;
                }
                app.run_state.should_exit = true;
                return Ok(true);
            }

            if matches!(command, AppClientCommand::SubmitTurn(_))
                && !app.console_state.can_submit_turn()
            {
                app.push_cell(HistoryCell::from_message(
                    "session",
                    "turn already running; wait, answer approval, or interrupt first",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }

            if let AppClientCommand::ApprovalResponse { .. } = &command {
                app.console_state.mode = FrontendMode::Running;
                app.approval_state.pending_request_id = None;
                app.input_pane.clear_views();
            }
            if let AppClientCommand::ResetSession { .. } = &command {
                app.reset_local_view();
                client.send_command(command)?;
                return Ok(false);
            }
            if let AppClientCommand::SubmitTurn(UserTurnInput { content, .. }) = &command {
                app.console_state.mode = FrontendMode::Running;
                app.run_state.status_notice = Some("Submitting turn".to_string());
                app.input_pane.clear_views();
                app.push_cell(HistoryCell::from_message(
                    "you",
                    content.clone(),
                    HistoryTone::User,
                ));
                app.run_state.last_message_count = app.run_state.last_message_count.saturating_add(1);
            }
            client.send_command(command)?;
        }
        ParsedInput::ApprovalAnswer { approved, reason } => {
            let Some(request_id) = app.approval_state.pending_request_id.clone() else {
                app.push_cell(HistoryCell::from_message(
                    "approval",
                    "no pending approval request",
                    HistoryTone::Error,
                ));
                return Ok(false);
            };
            app.console_state.mode = FrontendMode::Running;
            app.approval_state.pending_request_id = None;
            app.input_pane.clear_views();
            app.push_cell(HistoryCell::from_message(
                "approval",
                if approved { "approved" } else { "denied" },
                if approved {
                    HistoryTone::Agent
                } else {
                    HistoryTone::Warning
                },
            ));
            client.send_command(AppClientCommand::ApprovalResponse {
                session_id: session_id.to_string(),
                request_id,
                approved,
                reason: Some(reason),
            })?;
        }
    }
    Ok(false)
}

pub(crate) fn execute_server_action(app: &mut TuiApp, action: ServerAction) {
    match action {
        ServerAction::SetMode(mode) => {
            app.set_mode(mode);
        }
        ServerAction::SetPendingApproval(request_id) => {
            app.approval_state.pending_request_id = request_id;
        }
        ServerAction::SetStatusNotice(notice) => {
            app.run_state.status_notice = notice;
        }
        ServerAction::SetLastMessageCount(count) => {
            app.run_state.last_message_count = count;
        }
        ServerAction::SetHistoryLoaded(loaded) => {
            app.run_state.history_loaded = loaded;
        }
        ServerAction::ClearApprovalView => {
            app.input_pane.clear_approval();
        }
        ServerAction::ClearLastToolName => {
            app.run_state.last_tool_name = None;
        }
        ServerAction::ReplaceHistory(messages) => {
            app.transcript_state.transcript.replace_with_history(&messages);
            app.transcript_state.scroll = 0;
            app.clamp_transcript_scroll();
        }
        ServerAction::PushErrorCell(message) => {
            app.input_pane.clear_views();
            app.push_cell(HistoryCell::from_message("error", message, HistoryTone::Error));
        }
        ServerAction::ItemDispatch(dispatch) => match dispatch {
            ItemDispatch::AssistantStarted { turn_id, item_id } => {
                app.handle_assistant_item_started(&turn_id, &item_id);
            }
            ItemDispatch::ToolLikeStarted { item_id, title } => {
                app.handle_tool_item_started(&item_id, &title);
            }
            ItemDispatch::AssistantDelta { item_id, delta } => {
                app.handle_assistant_item_delta(&item_id, &delta);
            }
            ItemDispatch::AssistantCompleted { item_id } => {
                app.handle_assistant_item_completed(&item_id);
            }
            ItemDispatch::ToolLikeCompleted { item_id } => {
                app.handle_tool_item_completed(&item_id);
            }
        },
        ServerAction::TurnDispatch(dispatch) => app.apply_turn_dispatch(dispatch),
        ServerAction::ShowApprovalPrompt {
            title,
            detail,
            notice,
        } => {
            app.input_pane
                .set_approval(crate::ui::widgets::input_pane::ApprovalInlineState {
                    title,
                    detail,
                });
            app.run_state.status_notice = Some(notice);
        }
    }
}
