use crate::app::TuiApp;
use crate::app::cli_settings::{PersistedCliSettings, save_cli_settings};
use crate::app::commands::filter_toggle::apply_filter_toggle;
use crate::app::commands::parse::ParsedInput;
use crate::app::commands::permissions_mode::apply_permission_mode;
use crate::app::conversation::facade as conversation_facade;
use crate::app::effects::copy_text_to_clipboard;
use crate::input::slash_command::slash_command_help_text;
use crate::state::NoticeLevel;
use crate::state::reducer::ServerAction;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryFormat, HistoryTone};
use agent_app_server_client::AppServerClient;
use agent_core::{ServerRequestDecision, ServerRequestDecisionKind};
use agent_protocol::{AppClientCommand, UserTurnInput};
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
            let Some(text) = app.transcript_owner.last_copyable_output() else {
                app.push_live_cell(HistoryCell::info(
                    "conversation",
                    "`/copy` unavailable before first assistant output",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            };
            match copy_text_to_clipboard(text) {
                Ok(()) => {
                    app.push_live_cell(HistoryCell::info(
                        "conversation",
                        "Copied latest assistant output",
                        HistoryTone::Control,
                    ));
                }
                Err(err) => {
                    app.push_live_cell(HistoryCell::info(
                        "error",
                        format!("failed to copy: {err}"),
                        HistoryTone::Error,
                    ));
                }
            }
        }
        ParsedInput::LocalCopyText(text) => match copy_text_to_clipboard(&text) {
            Ok(()) => {
                app.push_live_cell(HistoryCell::info(
                    "conversation",
                    "Copied selected input text",
                    HistoryTone::Control,
                ));
            }
            Err(err) => {
                app.push_live_cell(HistoryCell::info(
                    "error",
                    format!("failed to copy: {err}"),
                    HistoryTone::Error,
                ));
            }
        },
        ParsedInput::LocalHelp => {
            app.push_live_cell(HistoryCell::agent(
                "commands",
                slash_command_help_text(),
                HistoryFormat::Markdown,
            ));
        }
        ParsedInput::LocalInputError(message) => {
            app.push_live_cell(HistoryCell::info(
                "conversation",
                message,
                HistoryTone::Warning,
            ));
        }
        ParsedInput::LocalPermissionMode(mode) => {
            if mode.trim().is_empty() {
                let current = app.run_state.permission_mode.clone();
                app.bottom_pane.set_permissions_picker(&current);
                return Ok(false);
            }
            if let Err(err) = apply_permission_mode(app, mode.trim()) {
                app.push_live_cell(HistoryCell::info("conversation", err, HistoryTone::Warning));
            } else if let Err(err) = persist_cli_settings(app) {
                app.push_live_cell(HistoryCell::info(
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
                app.bottom_pane
                    .set_config_panel(cfg.llm.api_key, cfg.llm.base_url, cfg.llm.model);
                return Ok(false);
            }
            if base_url.trim().is_empty() || model.trim().is_empty() {
                app.push_live_cell(HistoryCell::info(
                    "config",
                    "Base URL and Model cannot be empty.",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }
            save_user_llm_config(&api_key, &base_url, &model)?;
            app.push_live_cell(HistoryCell::info(
                "config",
                "Saved API Key / Base URL / Model to ~/.cloudagent/config.toml.",
                HistoryTone::Control,
            ));
            return Ok(false);
        }
        ParsedInput::LocalConversationCreate(new_conversation_id) => {
            app.bottom_pane.clear_session_picker();
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
            client.send_command(AppClientCommand::SwitchConversation {
                conversation_id: conversation_id.clone(),
            })?;
        }
        ParsedInput::LocalConversationSwitch(target_conversation_id) => {
            app.bottom_pane.clear_session_picker();
            let trimmed = target_conversation_id.trim();
            if trimmed.is_empty() {
                app.push_live_cell(HistoryCell::info(
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
                app.push_live_cell(HistoryCell::info(
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
                app.push_live_cell(HistoryCell::info(
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
                app.bottom_pane.request_session_picker(
                    crate::ui::widgets::session_picker::SessionPickerMode::Delete,
                );
                client.send_command(AppClientCommand::ListConversations)?;
                return Ok(false);
            }
            client.send_command(AppClientCommand::DeleteConversation {
                conversation_id: trimmed.to_string(),
            })?;
        }
        ParsedInput::LocalFilterToggle(raw_args) => {
            if raw_args.trim().is_empty() {
                app.bottom_pane.set_filter_picker();
                return Ok(false);
            }
            if let Err(usage) = apply_filter_toggle(app, &raw_args) {
                app.push_live_cell(HistoryCell::info(
                    "conversation",
                    usage,
                    HistoryTone::Warning,
                ));
                return Ok(false);
            } else if let Err(err) = persist_cli_settings(app) {
                app.push_live_cell(HistoryCell::info(
                    "config",
                    format!("failed to persist filter setting: {err}"),
                    HistoryTone::Warning,
                ));
            }
        }
        ParsedInput::Command(command) => {
            if let AppClientCommand::Exit = command {
                if app.current_mode() != agent_protocol::FrontendMode::Idle {
                    client.send_command(AppClientCommand::InterruptTurn {
                        conversation_id: app.conversation_id.clone(),
                    })?;
                }
                app.run_state.should_exit = true;
                return Ok(true);
            }

            if matches!(command, AppClientCommand::SubmitTurn(_)) && !app.can_submit_turn() {
                app.push_live_cell(HistoryCell::info(
                    "conversation",
                    "turn already running; wait, answer the pending request, or interrupt first",
                    HistoryTone::Warning,
                ));
                return Ok(false);
            }

            if let AppClientCommand::ResolveServerRequest { .. } = &command {
                app.push_live_cell(HistoryCell::info(
                    "request",
                    "server requests must be answered through the active approval view",
                    HistoryTone::Error,
                ));
                return Ok(false);
            }
            if let AppClientCommand::ResetConversation { .. } = &command {
                app.reset_local_view();
                client.send_command(command)?;
                app.arm_reset_notice_suppression();
                return Ok(false);
            }
            if let AppClientCommand::SubmitTurn(UserTurnInput { content, .. }) = &command {
                app.prepare_submitted_turn(content);
            }
            match command {
                AppClientCommand::SubmitTurn(input) => client.submit_turn(input)?,
                AppClientCommand::InterruptTurn { conversation_id } => {
                    client.interrupt_turn(conversation_id)?
                }
                AppClientCommand::ListConversations => client.list_conversations()?,
                other => client.send_command(other)?,
            }
        }
        ParsedInput::ServerRequestAnswer {
            request_id,
            decision,
            reason,
        } => {
            app.push_live_cell(HistoryCell::info(
                "request",
                format!("Request {}", decision_label(&decision)),
                HistoryTone::Control,
            ));
            client.resolve_server_request(
                app.conversation_id.clone(),
                request_id,
                ServerRequestDecision {
                    decision,
                    reason: Some(reason),
                },
            )?;
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
        ServerAction::SetConversationList(conversations) => {
            app.handle_conversation_list(conversations);
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
        ServerAction::ClearServerRequestView => {
            app.clear_server_request_view();
        }
        ServerAction::DismissServerRequestView(request_id) => {
            app.dismiss_server_request_view(&request_id);
        }
        ServerAction::ClearServerRequestStatus => {
            app.bottom_pane.clear_server_request();
        }
        ServerAction::ClearLastToolName => {
            app.on_server_tool_finished();
        }
        ServerAction::ReplaceHistory(messages) => {
            app.run_state.history_snapshot = Some(messages);
            conversation_facade::rebuild_transcript_from_history(app);
        }
        ServerAction::UpsertTurnSnapshot(turn) => {
            conversation_facade::upsert_turn_snapshot(app, turn);
        }
        ServerAction::BindActiveTurn(turn_id) => {
            app.transcript_owner
                .bind_turn_id(turn_id, app.run_state.expand_tool_details);
        }
        ServerAction::StartActiveTurnItem {
            turn_id,
            item_id,
            kind,
            title,
        } => {
            app.on_server_active_item_started(&kind, title.as_deref());
            app.transcript_owner.start_item(
                turn_id,
                item_id,
                kind,
                title,
                app.run_state.expand_tool_details,
            );
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
        ServerAction::AppendActiveOutputDelta {
            turn_id,
            item_id,
            delta,
        } => {
            app.transcript_owner.append_output_delta(
                turn_id,
                item_id,
                delta,
                app.run_state.expand_tool_details,
            );
        }
        ServerAction::CompleteActiveTurnItem {
            turn_id,
            item_id,
            item,
        } => {
            app.transcript_owner.complete_item(
                turn_id,
                item_id,
                item,
                app.run_state.expand_tool_details,
            );
            app.terminal_projection.on_stream_boundary();
        }
        ServerAction::PushNoticeCell {
            label,
            message,
            level,
        } => {
            if app.should_suppress_notice(&label, &message) {
                return;
            }
            let tone = match level {
                NoticeLevel::Info => HistoryTone::Control,
                NoticeLevel::Warn => HistoryTone::Warning,
                NoticeLevel::Error => HistoryTone::Error,
            };
            app.push_live_cell(HistoryCell::info(label, message, tone));
        }
        ServerAction::PushErrorCell(message) => {
            app.bottom_pane.clear_views();
            app.push_live_cell(HistoryCell::info("error", message, HistoryTone::Error));
        }
        ServerAction::TurnDispatch(dispatch) => {
            conversation_facade::apply_turn_dispatch(app, dispatch);
            app.terminal_projection.on_stream_boundary();
        }
        ServerAction::ShowServerRequestPrompt {
            request_id,
            title,
            detail,
            notice,
        } => {
            app.show_server_request_prompt(
                crate::ui::widgets::input_pane::ServerRequestInlineState {
                    request_id,
                    title,
                    detail,
                },
            );
            app.push_live_cell(HistoryCell::info("request", notice, HistoryTone::Warning));
        }
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
