use crate::app::TuiApp;
use crate::app::effects::copy_text_to_clipboard;
use crate::app::parse::ParsedInput;
use crate::input::slash_command::slash_command_help_text;
use crate::state::reducer::{ItemDispatch, ServerAction};
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone, render_history_entry};
use agent_app_server_client::AppServerClient;
use agent_protocol::{
    AppClientCommand, FrontendMode, ServerRequestDecision, ServerRequestDecisionKind,
    TranscriptItem, UserTurnInput,
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
            let trimmed = new_conversation_id.trim();
            if trimmed.is_empty() {
                app.push_cell(HistoryCell::from_message(
                    "conversation",
                    "Usage: /new <conversation-id>",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }
            client.send_command(AppClientCommand::CreateConversation {
                conversation_id: trimmed.to_string(),
            })?;
            client.send_command(AppClientCommand::SwitchConversation {
                conversation_id: trimmed.to_string(),
            })?;
        }
        ParsedInput::LocalConversationSwitch(target_conversation_id) => {
            let trimmed = target_conversation_id.trim();
            if trimmed.is_empty() {
                app.push_cell(HistoryCell::from_message(
                    "conversation",
                    "Usage: /session <conversation-id>",
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
                    "Usage: /archive <conversation-id>",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }
            client.send_command(AppClientCommand::ArchiveConversation {
                conversation_id: trimmed.to_string(),
            })?;
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
                app.run_state.status_notice = Some("Submitting turn".to_string());
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
            app.run_state.status_notice = Some(format!("Request {}", decision_label(&decision)));
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
    match action {
        ServerAction::SetMode(mode) => {
            app.set_mode(mode);
        }
        ServerAction::SetStatusNotice(notice) => {
            app.run_state.status_notice = notice;
        }
        ServerAction::SetConversationList(conversations) => {
            app.set_conversation_summaries(conversations);
        }
        ServerAction::SwitchConversation(conversation_id) => {
            app.switch_conversation(conversation_id);
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
            app.run_state.current_tool_activity = None;
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
        ServerAction::PushInfoCell(message) => {
            app.input_pane.clear_views();
            app.push_cell(HistoryCell::from_message(
                "context",
                message,
                HistoryTone::Control,
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
                if let TranscriptItem::AgentMessage { id, text } = item {
                    app.handle_assistant_item_completed(&id, &text);
                }
            }
            ItemDispatch::ReasoningCompleted { item } => match item {
                TranscriptItem::Reasoning { id, text, .. } => {
                    app.handle_reasoning_item_completed(&id, "reasoning", &text);
                }
                TranscriptItem::UserMessage { .. }
                | TranscriptItem::SystemMessage { .. }
                | TranscriptItem::AgentMessage { .. }
                | TranscriptItem::CommandExecution { .. }
                | TranscriptItem::FileChange { .. }
                | TranscriptItem::ToolResult { .. } => {}
            },
            ItemDispatch::ControlCompleted { item } => match item {
                TranscriptItem::CommandExecution { ref id, .. }
                | TranscriptItem::FileChange { ref id, .. }
                | TranscriptItem::ToolResult { ref id, .. } => {
                    app.handle_control_item_completed(id, render_history_entry(&item));
                }
                TranscriptItem::UserMessage { .. }
                | TranscriptItem::SystemMessage { .. }
                | TranscriptItem::AgentMessage { .. }
                | TranscriptItem::Reasoning { .. } => {}
            },
        },
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
            app.run_state.status_notice = Some(notice);
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

fn rebuild_transcript_from_history(app: &mut TuiApp) {
    app.transcript_state = crate::state::TranscriptState::default();
    app.input_pane.clear_views();

    let history_snapshot = app.run_state.history_snapshot.clone().unwrap_or_default();
    if !history_snapshot.is_empty() {
        let cells = history_snapshot
            .iter()
            .flat_map(|turn| turn.items.iter())
            .map(render_history_entry)
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>();
        app.replace_history_cells(cells);
        app.transcript_state.last_copyable_output = history_snapshot
            .iter()
            .rev()
            .flat_map(|turn| turn.items.iter().rev())
            .find_map(|entry| {
                if let agent_protocol::TranscriptItem::AgentMessage { text, .. } = entry {
                    (!text.trim().is_empty()).then(|| text.clone())
                } else {
                    None
                }
            });
    }
    app.run_state.history_loaded = app.run_state.history_snapshot.is_some();
}
