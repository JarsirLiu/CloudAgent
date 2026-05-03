use crate::app::TuiApp;
use crate::app::conversation_facade;
use crate::app::effects::copy_text_to_clipboard;
use crate::app::filter_toggle::apply_filter_toggle;
use crate::app::parse::ParsedInput;
use crate::app::runtime_updates::apply_runtime_projection_update;
use crate::input::slash_command::slash_command_help_text;
use crate::state::reducer::ServerAction;
use crate::state::NoticeLevel;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use agent_app_server_client::AppServerClient;
use agent_protocol::{
    AppClientCommand, FrontendMode, ServerRequestDecision, ServerRequestDecisionKind,
    UserTurnInput,
};
use anyhow::Result;

pub(crate) fn handle_tui_input(
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
                    app.run_state.set_system_notice_level(
                        "Copied latest assistant output",
                        NoticeLevel::Info,
                    );
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
        ParsedInput::LocalHelp => {
            app.push_cell(HistoryCell::from_message(
                "commands",
                slash_command_help_text(),
                HistoryTone::Agent,
            ));
        }
        ParsedInput::LocalInputError(message) => {
            app.push_cell(HistoryCell::from_message(
                "conversation",
                message,
                HistoryTone::Warning,
            ));
        }
        ParsedInput::LocalConversationCreate(new_conversation_id) => {
            app.input_pane.clear_session_picker();
            let trimmed = new_conversation_id.trim();
            let conversation_id = if trimmed.is_empty() {
                let millis = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                format!("session-{millis}")
            } else {
                trimmed.to_string()
            };
            client.send_command(AppClientCommand::CreateConversation {
                conversation_id: conversation_id.clone(),
            })?;
            client.send_command(AppClientCommand::SwitchConversation {
                conversation_id: conversation_id.clone(),
            })?;
        }
        ParsedInput::LocalConversationSwitch(target_conversation_id) => {
            app.input_pane.clear_session_picker();
            let trimmed = target_conversation_id.trim();
            if trimmed.is_empty() {
                app.push_cell(HistoryCell::from_message(
                    "conversation",
                    "Usage: /session <session-id>",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }
            client.send_command(AppClientCommand::SwitchConversation {
                conversation_id: trimmed.to_string(),
            })?;
        }
        ParsedInput::LocalConversationTitle(title) => {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                app.push_cell(HistoryCell::from_message(
                    "conversation",
                    "Usage: /title <text>",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }
            client.send_command(AppClientCommand::SetConversationTitle {
                conversation_id: app.conversation_id.clone(),
                title: trimmed.to_string(),
            })?;
        }
        ParsedInput::LocalConversationArchive(target_conversation_id) => {
            let trimmed = target_conversation_id.trim();
            if trimmed.is_empty() {
                app.push_cell(HistoryCell::from_message(
                    "conversation",
                    "Usage: /archive <session-id>",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }
            client.send_command(AppClientCommand::ArchiveConversation {
                conversation_id: trimmed.to_string(),
            })?;
        }
        ParsedInput::LocalFilterToggle(raw_args) => {
            if let Err(usage) = apply_filter_toggle(app, &raw_args) {
                app.push_cell(HistoryCell::from_message(
                    "conversation",
                    usage,
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }
        }
        ParsedInput::Command(command) => {
            if let AppClientCommand::Exit = command {
                if app.console_state.mode != FrontendMode::Idle {
                    client.send_command(AppClientCommand::InterruptTurn {
                        conversation_id: app.conversation_id.clone(),
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
                app.push_cell(HistoryCell::from_message(
                    "request",
                    "server requests must be answered through the active approval view",
                    HistoryTone::Error,
                ));
                return Ok(false);
            }
            if let AppClientCommand::ResetConversation { .. } = &command {
                app.reset_local_view();
                client.send_command(command)?;
                return Ok(false);
            }
            if let AppClientCommand::SubmitTurn(UserTurnInput { content, .. }) = &command {
                app.console_state.mode = FrontendMode::Running;
                app.run_state
                    .set_system_notice_level("Submitting turn", NoticeLevel::Info);
                app.run_state.last_turn_usage = None;
                app.run_state.total_turn_usage = None;
                app.run_state.model_context_window = None;
                app.input_pane.clear_views();
                app.push_cell(HistoryCell::from_message(
                    "you",
                    content.clone(),
                    HistoryTone::User,
                ));
            }
            client.send_command(command)?;
        }
        ParsedInput::ServerRequestAnswer {
            request_id,
            decision,
            reason,
        } => {
            sync_mode_after_server_request_view(app);
            app.run_state.set_system_notice_level(
                format!("Request {}", decision_label(&decision)),
                NoticeLevel::Info,
            );
            client.send_command(AppClientCommand::ResolveServerRequest {
                conversation_id: app.conversation_id.clone(),
                request_id,
                decision: ServerRequestDecision {
                    decision,
                    reason: Some(reason),
                },
            })?;
        }
    }
    Ok(false)
}

fn decision_label(decision: &ServerRequestDecisionKind) -> &'static str {
    match decision {
        ServerRequestDecisionKind::Accept => "approved",
        ServerRequestDecisionKind::AcceptForSession => "approved for session",
        ServerRequestDecisionKind::Decline => "denied",
        ServerRequestDecisionKind::Cancel => "cancelled",
    }
}

pub(crate) fn execute_server_action(app: &mut TuiApp, action: ServerAction) {
    apply_runtime_projection_update(app, &action);
    match action {
        ServerAction::SetMode(mode) => {
            app.set_mode(mode);
        }
        ServerAction::SetSystemNotice { text, level } => {
            app.run_state.set_system_notice_level(text, level);
        }
        ServerAction::ClearSystemNotice => app.run_state.clear_system_notice(),
        ServerAction::SetConversationList(conversations) => {
            app.set_conversation_summaries(conversations.clone());
            if app.session_picker_requested {
                app.input_pane
                    .set_session_picker(conversations, &app.conversation_id);
                app.session_picker_requested = false;
            }
        }
        ServerAction::SwitchConversation(conversation_id) => {
            app.input_pane.clear_session_picker();
            app.switch_conversation(conversation_id);
            app.session_picker_requested = false;
        }
        ServerAction::SetHistoryLoaded(loaded) => {
            app.run_state.history_loaded = loaded;
        }
        ServerAction::ClearCurrentTurnUsage => {
            app.run_state.last_turn_usage = None;
            app.run_state.total_turn_usage = None;
            app.run_state.model_context_window = None;
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
        ServerAction::ClearServerRequestView => {
            app.input_pane.clear_server_request();
        }
        ServerAction::DismissServerRequestView(request_id) => {
            app.input_pane.dismiss_server_request(&request_id);
            sync_mode_after_server_request_view(app);
        }
        ServerAction::ClearServerRequestStatus => {
            app.server_request_state.active_request_id = None;
            app.server_request_state.action_required = false;
        }
        ServerAction::ClearLastToolName => {
        }
        ServerAction::ReplaceHistory(messages) => {
            app.run_state.history_snapshot = Some(messages);
            conversation_facade::rebuild_transcript_from_history(app);
        }
        ServerAction::PushErrorCell(message) => {
            app.input_pane.clear_views();
            app.push_cell(HistoryCell::from_message(
                "error",
                message,
                HistoryTone::Error,
            ));
        }
        ServerAction::PushInfoCell(message) => {
            app.push_cell(HistoryCell::from_message(
                "context",
                message,
                HistoryTone::Control,
            ));
        }
        ServerAction::ItemDispatch(dispatch) => conversation_facade::apply_item_dispatch(app, dispatch),
        ServerAction::TurnDispatch(dispatch) => app.apply_turn_dispatch(dispatch),
        ServerAction::ShowServerRequestPrompt {
            request_id,
            title,
            detail,
            notice,
        } => {
            app.input_pane.set_server_request(
                crate::ui::widgets::input_pane::ServerRequestInlineState {
                    request_id,
                    title,
                    detail,
                },
            );
            sync_mode_after_server_request_view(app);
            app.run_state
                .set_system_notice_level(notice, NoticeLevel::Warn);
        }
    }
}

fn sync_mode_after_server_request_view(app: &mut TuiApp) {
    if app.input_pane.requires_action() {
        app.console_state.mode = FrontendMode::WaitingForServerRequest;
        app.server_request_state.active_request_id = app.input_pane.active_server_request_id();
        app.server_request_state.action_required = true;
    } else {
        app.console_state.mode = FrontendMode::Running;
        app.server_request_state.active_request_id = None;
        app.server_request_state.action_required = false;
    }
}
