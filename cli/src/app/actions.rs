use crate::app::TuiApp;
use crate::app::effects::copy_text_to_clipboard;
use crate::app::parse::ParsedInput;
use crate::state::reducer::{ItemDispatch, ServerAction};
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use agent_app_server_client::AppServerClient;
use agent_protocol::{AppClientCommand, FrontendMode, ThreadItem, TurnItemKind, UserTurnInput};
use anyhow::Result;

pub(crate) fn handle_tui_input(
    conversation_id: &str,
    app: &mut TuiApp,
    client: &AppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::LocalCopy => {
            let Some(text) = app.transcript_state.last_copyable_output.as_deref() else {
                app.push_cell(HistoryCell::from_message(
                    "conversation",
                    "`/copy` unavailable before first assistant output",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            };
            match copy_text_to_clipboard(text) {
                Ok(()) => {
                    app.run_state.status_notice =
                        Some("Copied latest assistant output".to_string());
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
                        conversation_id: conversation_id.to_string(),
                    })?;
                }
                app.run_state.should_exit = true;
                return Ok(true);
            }

            if matches!(command, AppClientCommand::SubmitTurn(_))
                && !app.console_state.can_submit_turn()
            {
                app.push_cell(HistoryCell::from_message(
                    "conversation",
                    "turn already running; wait, answer the pending request, or interrupt first",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }

            if let AppClientCommand::ResolveServerRequest { .. } = &command {
                app.console_state.mode = FrontendMode::Running;
                app.server_request_state.pending_server_request_id = None;
                app.input_pane.clear_views();
            }
            if let AppClientCommand::ResetConversation { .. } = &command {
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
                app.run_state.last_message_count =
                    app.run_state.last_message_count.saturating_add(1);
            }
            client.send_command(command)?;
        }
        ParsedInput::ServerRequestAnswer { approved, reason } => {
            let Some(request_id) = app.server_request_state.pending_server_request_id.clone()
            else {
                app.push_cell(HistoryCell::from_message(
                    "request",
                    "no pending server request",
                    HistoryTone::Error,
                ));
                return Ok(false);
            };
            app.console_state.mode = FrontendMode::Running;
            app.server_request_state.pending_server_request_id = None;
            app.input_pane.clear_views();
            app.push_cell(HistoryCell::from_message(
                "request",
                if approved { "approved" } else { "denied" },
                if approved {
                    HistoryTone::Agent
                } else {
                    HistoryTone::Warning
                },
            ));
            client.send_command(AppClientCommand::ResolveServerRequest {
                conversation_id: conversation_id.to_string(),
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
        ServerAction::SetPendingServerRequest(request_id) => {
            app.server_request_state.pending_server_request_id = request_id;
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
        ServerAction::ClearServerRequestView => {
            app.input_pane.clear_server_request();
        }
        ServerAction::ClearLastToolName => {
            app.run_state.last_tool_name = None;
        }
        ServerAction::ReplaceHistory(messages) => {
            app.run_state.history_snapshot = Some(messages);
            rebuild_transcript_from_history(app);
        }
        ServerAction::PushErrorCell(message) => {
            app.input_pane.clear_views();
            app.push_cell(HistoryCell::from_message(
                "error",
                message,
                HistoryTone::Error,
            ));
        }
        ServerAction::ItemDispatch(dispatch) => match dispatch {
            ItemDispatch::AssistantStarted { turn_id, item_id } => {
                app.handle_assistant_item_started(&turn_id, &item_id);
            }
            ItemDispatch::ReasoningStarted { item_id, title } => {
                app.handle_reasoning_item_started(&item_id, &title);
            }
            ItemDispatch::ControlStarted {
                item_id,
                kind,
                title,
            } => {
                app.handle_control_item_started(&item_id, kind, &title);
            }
            ItemDispatch::AssistantDelta { item_id, delta } => {
                app.handle_assistant_item_delta(&item_id, &delta);
            }
            ItemDispatch::ReasoningDelta { item_id, delta } => {
                app.handle_reasoning_item_delta(&item_id, &delta);
            }
            ItemDispatch::ControlDelta { item_id, delta } => {
                app.handle_control_item_delta(&item_id, &delta);
            }
            ItemDispatch::AssistantCompleted { item } => {
                if let ThreadItem::AgentMessage { id, text } = item {
                    app.handle_assistant_item_completed(&id, &text);
                }
            }
            ItemDispatch::ReasoningCompleted { item } => match item {
                ThreadItem::Reasoning { id, text, .. } => {
                    app.handle_reasoning_item_completed(&id, "reasoning", &text);
                }
                ThreadItem::UserMessage { .. }
                | ThreadItem::AgentMessage { .. }
                | ThreadItem::CommandExecution { .. }
                | ThreadItem::ToolResult { .. } => {}
            },
            ItemDispatch::ControlCompleted { item } => match item {
                ThreadItem::CommandExecution {
                    id,
                    command,
                    tool_name,
                    summary,
                    ..
                } => {
                    let title = if command.trim().is_empty() {
                        tool_name.as_str()
                    } else {
                        command.as_str()
                    };
                    app.handle_control_item_completed(
                        &id,
                        TurnItemKind::CommandExecution,
                        title,
                        &summary,
                    );
                }
                ThreadItem::ToolResult {
                    id,
                    tool_name,
                    summary,
                    ..
                } => {
                    app.handle_control_item_completed(
                        &id,
                        TurnItemKind::ToolResult,
                        &tool_name,
                        &summary,
                    );
                }
                ThreadItem::UserMessage { .. }
                | ThreadItem::AgentMessage { .. }
                | ThreadItem::Reasoning { .. } => {}
            },
        },
        ServerAction::TurnDispatch(dispatch) => app.apply_turn_dispatch(dispatch),
        ServerAction::ShowServerRequestPrompt {
            title,
            detail,
            notice,
        } => {
            app.input_pane.set_server_request(
                crate::ui::widgets::input_pane::ServerRequestInlineState { title, detail },
            );
            app.run_state.status_notice = Some(notice);
        }
    }
}

fn rebuild_transcript_from_history(app: &mut TuiApp) {
    app.transcript_state = crate::state::TranscriptState::default();
    app.input_pane.clear_views();

    let history_snapshot = app.run_state.history_snapshot.clone().unwrap_or_default();
    if !history_snapshot.is_empty() {
        app.transcript_state
            .transcript
            .replace_with_history(&history_snapshot);
        app.transcript_state.last_copyable_output =
            history_snapshot.iter().rev().find_map(|entry| {
                if let agent_protocol::HistoryEntry::Assistant { content, .. } = entry {
                    content.clone().filter(|text| !text.trim().is_empty())
                } else {
                    None
                }
            });
    }
    app.run_state.history_loaded = app.run_state.history_snapshot.is_some();
    app.clamp_transcript_scroll();
}
