use crate::app::TuiApp;
use crate::app::cli_settings::{PersistedCliSettings, save_cli_settings};
use crate::app::commands::filter_toggle::apply_filter_toggle;
use crate::app::commands::parse::ParsedInput;
use crate::app::commands::permissions_mode::apply_permission_mode;
use crate::app::conversation::facade as conversation_facade;
use crate::app::effects::copy_text_to_clipboard;
use crate::input::slash_command::slash_command_help_text;
use crate::state::NoticeLevel;
use crate::state::WeixinBindingState;
use crate::state::reducer::ServerAction;
use crate::ui::widgets::gateway_panel::WeixinLoginSessionView;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryFormat};
use crate::ui::widgets::weixin_binding_view::WeixinBindingViewModel;
use agent_app_server_client::AppServerClient;
use agent_core::{ServerRequestDecision, ServerRequestDecisionKind};
use agent_protocol::{AppClientCommand, UserTurnInput};
use anyhow::Result;
use config::AgentConfig;
use std::fmt::Display;
use std::fs;

fn show_local_notice(app: &mut TuiApp, level: NoticeLevel, message: impl Into<String>) {
    app.bottom_pane.show_transient_notice(level, message.into());
}

fn platform_request_notice(action: &str, err: &impl Display) -> String {
    let detail = err.to_string();
    if detail.contains("unsupported request method: platform/") {
        return format!(
            "Platform management is unavailable on the connected node while trying to {action}. \
Restart the local gatewayd with the latest build, then try /gateway again."
        );
    }
    format!("Failed to {action}: {detail}")
}

async fn reload_gateway_panel(
    app: &mut TuiApp,
    client: &AppServerClient,
    platform: &str,
    weixin_login: Option<WeixinLoginSessionView>,
) -> Result<bool> {
    let status = match client.request_platform_status_typed(platform).await {
        Ok(status) => status,
        Err(err) => {
            show_local_notice(
                app,
                NoticeLevel::Error,
                platform_request_notice("load platform status", &err),
            );
            return Ok(false);
        }
    };
    let config = match client.request_platform_config_typed(platform).await {
        Ok(config) => config,
        Err(err) => {
            show_local_notice(
                app,
                NoticeLevel::Error,
                platform_request_notice("load platform config", &err),
            );
            return Ok(false);
        }
    };
    if platform == "weixin" {
        app.bottom_pane.set_gateway_edit_panel_with_weixin_login(
            status.platform,
            config,
            weixin_login,
        );
    } else {
        app.bottom_pane
            .set_gateway_edit_panel(status.platform, config);
    }
    Ok(false)
}

pub(crate) async fn handle_tui_input(
    app: &mut TuiApp,
    client: &AppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::LocalCopy => {
            let Some(text) = app.transcript_owner.last_copyable_output() else {
                show_local_notice(
                    app,
                    NoticeLevel::Warn,
                    "`/copy` unavailable before first assistant output",
                );
                return Ok(false);
            };
            match copy_text_to_clipboard(text) {
                Ok(()) => {
                    show_local_notice(app, NoticeLevel::Info, "Copied latest assistant output");
                }
                Err(err) => {
                    show_local_notice(app, NoticeLevel::Error, format!("failed to copy: {err}"));
                }
            }
        }
        ParsedInput::LocalCopyText(text) => match copy_text_to_clipboard(&text) {
            Ok(()) => {
                show_local_notice(app, NoticeLevel::Info, "Copied selected input text");
            }
            Err(err) => {
                show_local_notice(app, NoticeLevel::Error, format!("failed to copy: {err}"));
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
            show_local_notice(app, NoticeLevel::Warn, message);
        }
        ParsedInput::LocalPermissionMode(mode) => {
            if mode.trim().is_empty() {
                let current = app.run_state.permission_mode.clone();
                app.bottom_pane.set_permissions_picker(&current);
                return Ok(false);
            }
            if let Err(err) = apply_permission_mode(app, mode.trim()) {
                show_local_notice(app, NoticeLevel::Warn, err);
            } else if let Err(err) = persist_cli_settings(app) {
                show_local_notice(
                    app,
                    NoticeLevel::Warn,
                    format!("failed to persist permission mode: {err}"),
                );
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
                show_local_notice(
                    app,
                    NoticeLevel::Warn,
                    "Base URL and Model cannot be empty.",
                );
                return Ok(false);
            }
            save_user_llm_config(&api_key, &base_url, &model)?;
            show_local_notice(
                app,
                NoticeLevel::Info,
                "Saved API Key / Base URL / Model to ~/.cloudagent/config.toml.",
            );
            return Ok(false);
        }
        ParsedInput::LocalGatewayOpen => {
            let response = match client.request_platform_list_typed().await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        NoticeLevel::Error,
                        platform_request_notice("load platform list", &err),
                    );
                    return Ok(false);
                }
            };
            app.bottom_pane.set_gateway_list_panel(response.platforms);
            return Ok(false);
        }
        ParsedInput::LocalWeixinLoginStart => {
            let response = match client.start_weixin_login_typed().await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        NoticeLevel::Error,
                        platform_request_notice("start weixin login", &err),
                    );
                    return Ok(false);
                }
            };
            app.push_live_cell(HistoryCell::agent(
                "weixin-login",
                format!(
                    "Weixin QR login started.\n\nSession: `{}`\n\nScan URL:\n{}\n\nAfter confirming on phone, run:\n`/weixin-login-check {}`",
                    response.session_id, response.qr_url, response.session_id
                ),
                HistoryFormat::Markdown,
            ));
            show_local_notice(
                app,
                NoticeLevel::Info,
                format!("Weixin login session `{}` started", response.session_id),
            );
            return Ok(false);
        }
        ParsedInput::LocalWeixinLoginCheck(session_id) => {
            let trimmed = session_id.trim();
            if trimmed.is_empty() {
                show_local_notice(
                    app,
                    NoticeLevel::Warn,
                    "Usage: /weixin-login-check <session-id>",
                );
                return Ok(false);
            }
            let response = match client.check_weixin_login_typed(trimmed).await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        NoticeLevel::Error,
                        platform_request_notice("check weixin login", &err),
                    );
                    return Ok(false);
                }
            };
            let message = match response.status.as_str() {
                "confirmed" => format!(
                    "Weixin login confirmed for `{}`. You can now open `/gateway` and enable `weixin`.",
                    response.account_id.as_deref().unwrap_or("unknown")
                ),
                "pending" => format!(
                    "Weixin login `{}` is still waiting for scan confirmation.",
                    response.session_id
                ),
                "expired" => "Weixin QR expired. Run `/weixin-login` again.".to_string(),
                _ => response
                    .message
                    .clone()
                    .unwrap_or_else(|| "Weixin login session not found.".to_string()),
            };
            app.push_live_cell(HistoryCell::agent(
                "weixin-login",
                message.clone(),
                HistoryFormat::Markdown,
            ));
            show_local_notice(app, NoticeLevel::Info, message);
            return Ok(false);
        }
        ParsedInput::LocalGatewaySelect(platform) => {
            return reload_gateway_panel(app, client, &platform, None).await;
        }
        ParsedInput::LocalGatewayWeixinLoginStart(platform) => {
            let response = match client.start_weixin_login_typed().await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        NoticeLevel::Error,
                        platform_request_notice("start weixin login", &err),
                    );
                    return Ok(false);
                }
            };
            app.push_live_cell(HistoryCell::agent(
                "weixin-login",
                format!(
                    "Weixin QR login started.\n\nSession: `{}`\n\nScan URL:\n{}",
                    response.session_id, response.qr_url
                ),
                HistoryFormat::Markdown,
            ));
            show_local_notice(
                app,
                NoticeLevel::Info,
                "Weixin QR login started. Scan with WeChat; this page will check automatically.",
            );
            app.run_state.weixin_binding = Some(WeixinBindingState {
                platform: platform.clone(),
                session_id: response.session_id.clone(),
                qr_url: response.qr_url.clone(),
                status: "waiting for scan".to_string(),
                next_poll_at: std::time::Instant::now() + std::time::Duration::from_secs(2),
            });
            app.bottom_pane
                .set_weixin_binding_view(WeixinBindingViewModel {
                    platform,
                    session_id: response.session_id,
                    qr_url: response.qr_url,
                    status: "waiting for scan".to_string(),
                });
            return Ok(false);
        }
        ParsedInput::LocalGatewayWeixinLoginCheck {
            platform,
            session_id,
            qr_url,
        } => {
            let response = match client.check_weixin_login_typed(&session_id).await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        NoticeLevel::Error,
                        platform_request_notice("check weixin login", &err),
                    );
                    return Ok(false);
                }
            };
            match response.status.as_str() {
                "confirmed" => {
                    app.run_state.weixin_binding = None;
                    let status = match client.set_platform_enabled_typed(&platform, true).await {
                        Ok(status) => status,
                        Err(err) => {
                            show_local_notice(
                                app,
                                NoticeLevel::Error,
                                platform_request_notice("enable weixin connection", &err),
                            );
                            return Ok(false);
                        }
                    };
                    show_local_notice(
                        app,
                        NoticeLevel::Info,
                        format!(
                            "Weixin connected as `{}`.",
                            response.account_id.as_deref().unwrap_or("unknown")
                        ),
                    );
                    let config = match client.request_platform_config_typed(&platform).await {
                        Ok(config) => config,
                        Err(err) => {
                            show_local_notice(
                                app,
                                NoticeLevel::Error,
                                platform_request_notice("reload platform config", &err),
                            );
                            return Ok(false);
                        }
                    };
                    app.bottom_pane
                        .set_gateway_edit_panel(status.platform, config);
                    return Ok(false);
                }
                "pending" => {
                    app.run_state.weixin_binding = Some(WeixinBindingState {
                        platform: platform.clone(),
                        session_id: session_id.clone(),
                        qr_url: qr_url.clone(),
                        status: "waiting for confirmation".to_string(),
                        next_poll_at: std::time::Instant::now() + std::time::Duration::from_secs(2),
                    });
                    app.bottom_pane
                        .set_weixin_binding_view(WeixinBindingViewModel {
                            platform,
                            session_id,
                            qr_url,
                            status: "waiting for confirmation".to_string(),
                        });
                    return Ok(false);
                }
                "expired" => {
                    app.run_state.weixin_binding = None;
                    show_local_notice(
                        app,
                        NoticeLevel::Warn,
                        "Weixin QR expired. Start Connection again.",
                    );
                    return reload_gateway_panel(app, client, &platform, None).await;
                }
                _ => {
                    app.run_state.weixin_binding = None;
                    show_local_notice(
                        app,
                        NoticeLevel::Warn,
                        response
                            .message
                            .unwrap_or_else(|| "Weixin login session not found.".to_string()),
                    );
                    return reload_gateway_panel(app, client, &platform, None).await;
                }
            }
        }
        ParsedInput::LocalGatewaySave {
            platform,
            enabled,
            updates,
        } => {
            for update in updates {
                let result = match update.value {
                    Some(value) => {
                        client
                            .set_platform_config_value_typed(&platform, update.key, value)
                            .await
                    }
                    None => {
                        client
                            .clear_platform_config_value_typed(&platform, update.key)
                            .await
                    }
                };
                if let Err(err) = result {
                    show_local_notice(
                        app,
                        NoticeLevel::Error,
                        platform_request_notice("save platform config", &err),
                    );
                    return Ok(false);
                }
            }
            let status = match client.set_platform_enabled_typed(&platform, enabled).await {
                Ok(status) => status,
                Err(err) => {
                    show_local_notice(
                        app,
                        NoticeLevel::Error,
                        platform_request_notice("update platform state", &err),
                    );
                    return Ok(false);
                }
            };
            let enabled_label = if status.platform.enabled {
                "enabled"
            } else {
                "disabled"
            };
            let config = match client.request_platform_config_typed(&platform).await {
                Ok(config) => config,
                Err(err) => {
                    show_local_notice(
                        app,
                        NoticeLevel::Error,
                        platform_request_notice("reload platform config", &err),
                    );
                    return Ok(false);
                }
            };
            app.bottom_pane
                .set_gateway_edit_panel(status.platform, config);
            show_local_notice(
                app,
                NoticeLevel::Info,
                format!(
                    "Saved gateway settings for `{platform}`; connection is now {}",
                    enabled_label
                ),
            );
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
                show_local_notice(app, NoticeLevel::Warn, "Usage: /session <session-id>");
                return Ok(false);
            }
            client.send_command(AppClientCommand::SwitchConversation {
                conversation_id: trimmed.to_string(),
            })?;
        }
        ParsedInput::LocalConversationTitle(title) => {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                show_local_notice(app, NoticeLevel::Warn, "Usage: /title <text>");
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
                show_local_notice(app, NoticeLevel::Warn, "Usage: /archive <session-id>");
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
                let response = client.request_conversation_list_typed().await?;
                execute_server_action(
                    app,
                    ServerAction::SetConversationList(response.conversations),
                );
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
                show_local_notice(app, NoticeLevel::Warn, usage);
                return Ok(false);
            } else if let Err(err) = persist_cli_settings(app) {
                show_local_notice(
                    app,
                    NoticeLevel::Warn,
                    format!("failed to persist filter setting: {err}"),
                );
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
                show_local_notice(
                    app,
                    NoticeLevel::Warn,
                    "turn already running; wait, answer the pending request, or interrupt first",
                );
                return Ok(false);
            }

            if let AppClientCommand::ResolveServerRequest { .. } = &command {
                show_local_notice(
                    app,
                    NoticeLevel::Error,
                    "server requests must be answered through the active approval view",
                );
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
                AppClientCommand::ListConversations => {
                    let response = client.request_conversation_list_typed().await?;
                    execute_server_action(
                        app,
                        ServerAction::SetConversationList(response.conversations),
                    );
                }
                other => client.send_command(other)?,
            }
        }
        ParsedInput::ServerRequestAnswer {
            request_id,
            decision,
            reason,
        } => {
            show_local_notice(
                app,
                NoticeLevel::Info,
                format!("Request {}", decision_label(&decision)),
            );
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
        ServerAction::SetFrontendMode(mode) => {
            app.sync_frontend_mode(mode);
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
            app.bottom_pane.show_transient_notice(level, message);
        }
        ServerAction::PushErrorCell(message) => {
            app.bottom_pane.clear_views();
            app.bottom_pane
                .show_transient_notice(NoticeLevel::Error, message);
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
            app.bottom_pane
                .show_transient_notice(NoticeLevel::Warn, notice);
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
