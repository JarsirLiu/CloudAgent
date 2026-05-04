use crate::app::TuiApp;
use crate::app::cli_settings::{PersistedCliSettings, save_cli_settings};
use crate::app::commands::filter_toggle::apply_filter_toggle;
use crate::app::commands::parse::ParsedInput;
use crate::app::commands::permissions_mode::apply_permission_mode;
use crate::app::conversation::facade as conversation_facade;
use crate::app::effects::copy_text_to_clipboard;
use crate::app::runtime::projection::apply_runtime_projection_update;
use crate::input::slash_command::slash_command_help_text;
use crate::state::NoticeLevel;
use crate::state::reducer::ServerAction;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use agent_app_server_client::AppServerClient;
use agent_protocol::{
    AppClientCommand, FrontendMode, ServerRequestDecision, ServerRequestDecisionKind, UserTurnInput,
};
use anyhow::Result;
use config::AgentConfig;
use std::fs;

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
        ParsedInput::LocalPermissionMode(mode) => {
            if mode.trim().is_empty() {
                let current = app.run_state.permission_mode.clone();
                app.input_pane.set_permissions_picker(&current);
                return Ok(false);
            }
            if let Err(err) = apply_permission_mode(app, mode.trim()) {
                app.push_cell(HistoryCell::from_message(
                    "conversation",
                    err,
                    HistoryTone::Warning,
                ));
            } else if let Err(err) = persist_cli_settings(app) {
                app.push_cell(HistoryCell::from_message(
                    "config",
                    format!("failed to persist permission mode: {err}"),
                    HistoryTone::Warning,
                ));
            }
            return Ok(false);
        }
        ParsedInput::LocalConfig {
            api_key,
            base_url,
            model,
        } => {
            if api_key.is_empty() && base_url.is_empty() && model.is_empty() {
                let cfg = AgentConfig::load_user_only(app.workspace_root.clone())?;
                app.input_pane
                    .set_config_panel(cfg.llm.api_key, cfg.llm.base_url, cfg.llm.model);
                return Ok(false);
            }
            if base_url.trim().is_empty() || model.trim().is_empty() {
                app.push_cell(HistoryCell::from_message(
                    "config",
                    "Base URL and Model cannot be empty.",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }
            save_user_llm_config(&api_key, &base_url, &model)?;
            app.run_state.set_system_notice_level(
                "Config updated in ~/.cloudagent/config.toml",
                NoticeLevel::Info,
            );
            app.push_cell(HistoryCell::from_message(
                "config",
                "Saved API Key / Base URL / Model.",
                HistoryTone::Control,
            ));
            return Ok(false);
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
        ParsedInput::LocalConversationDelete(target_conversation_id) => {
            let trimmed = target_conversation_id.trim();
            if trimmed.is_empty() {
                app.delete_picker_requested = true;
                app.session_picker_requested = false;
                client.send_command(AppClientCommand::ListConversations)?;
                return Ok(false);
            }
            client.send_command(AppClientCommand::DeleteConversation {
                conversation_id: trimmed.to_string(),
            })?;
        }
        ParsedInput::LocalFilterToggle(raw_args) => {
            if raw_args.trim().is_empty() {
                app.input_pane.set_filter_picker();
                return Ok(false);
            }
            if let Err(usage) = apply_filter_toggle(app, &raw_args) {
                app.push_cell(HistoryCell::from_message(
                    "conversation",
                    usage,
                    HistoryTone::Warning,
                ));
                return Ok(false);
            } else if let Err(err) = persist_cli_settings(app) {
                app.push_cell(HistoryCell::from_message(
                    "config",
                    format!("failed to persist filter setting: {err}"),
                    HistoryTone::Warning,
                ));
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
                app.input_pane.set_session_picker(
                    conversations,
                    &app.conversation_id,
                    crate::ui::widgets::session_picker::SessionPickerMode::Switch,
                );
                app.session_picker_requested = false;
                app.delete_picker_requested = false;
            } else if app.delete_picker_requested {
                app.input_pane.set_session_picker(
                    conversations,
                    &app.conversation_id,
                    crate::ui::widgets::session_picker::SessionPickerMode::Delete,
                );
                app.delete_picker_requested = false;
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
        ServerAction::SetRetryStatus { .. } => {}
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
        ServerAction::ClearLastToolName => {}
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
        ServerAction::ItemDispatch(dispatch) => {
            conversation_facade::apply_item_dispatch(app, dispatch)
        }
        ServerAction::TurnDispatch(dispatch) => {
            conversation_facade::apply_turn_dispatch(app, dispatch)
        }
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

fn persist_cli_settings(app: &TuiApp) -> Result<()> {
    save_cli_settings(
        &app.conversation_store_dir,
        &PersistedCliSettings::new(
            app.run_state.pre_llm_filter_enabled,
            app.run_state.permission_mode.clone(),
        ),
    )
}

fn save_user_llm_config(api_key: &str, base_url: &str, model: &str) -> Result<()> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("Cannot find user home directory"))?;
    let config_dir = std::path::PathBuf::from(home).join(".cloudagent");
    fs::create_dir_all(&config_dir)?;
    let path = config_dir.join("config.toml");
    let body = format!(
        "[llm]\napi_key = \"{}\"\nbase_url = \"{}\"\nmodel = \"{}\"\n",
        api_key.replace('"', "\\\""),
        base_url.replace('"', "\\\""),
        model.replace('"', "\\\"")
    );
    fs::write(path, body)?;
    Ok(())
}
