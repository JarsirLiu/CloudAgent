use crate::app::TuiApp;
use crate::app::cli_settings::{PersistedCliSettings, save_cli_settings};
use crate::app::commands::filter_toggle::apply_filter_toggle;
use crate::app::commands::parse::ParsedInput;
use crate::app::commands::permissions_mode::apply_permission_mode;
use crate::app::conversation::facade as conversation_facade;
use crate::app::conversation::image_paste::handle_clipboard_paste;
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
use config::{AgentConfig, ReasoningEffort};
use serde::Serialize;
use std::collections::HashSet;
use std::fmt::Display;
use std::fs;
use tokio::time::{Duration, timeout};

const HISTORY_PAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn show_local_notice(app: &mut TuiApp, level: NoticeLevel, message: impl Into<String>) {
    app.bottom_pane.show_transient_notice(level, message.into());
}

fn platform_request_notice(action: &str, err: &impl Display) -> String {
    let detail = err.to_string();
    if detail.contains("unsupported request method: platform/") {
        return format!(
            "Platform management is unavailable on the connected node while trying to {action}. \
Restart the local node with the latest build, then try /gateway again."
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
        ParsedInput::LocalImagePaste => {
            handle_clipboard_paste(app);
        }
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
            save_user_llm_config_default(&api_key, &base_url, &model)?;
            if let Err(err) = client.send_command(AppClientCommand::ReloadLlmConfig {
                api_key: api_key.clone(),
                base_url: base_url.clone(),
                model: model.clone(),
            }) {
                tracing::warn!("failed to reload LLM config after save: {err}");
            }
            show_local_notice(
                app,
                NoticeLevel::Info,
                "Saved API Key / Base URL / Model to ~/.cloudagent/config.toml.",
            );
            return Ok(false);
        }
        ParsedInput::LocalReasoning(effort) => {
            if effort.trim().is_empty() {
                let cfg = AgentConfig::load_user_only(app.workspace_root.clone())?;
                app.bottom_pane
                    .set_reasoning_picker(cfg.llm.model_reasoning_effort);
                return Ok(false);
            }
            let effort = match effort.trim().parse::<ReasoningEffort>() {
                Ok(effort) => effort,
                Err(_) => {
                    show_local_notice(
                        app,
                        NoticeLevel::Warn,
                        "Reasoning effort must be one of: low, medium, high.",
                    );
                    return Ok(false);
                }
            };
            let cfg = AgentConfig::load_user_only(app.workspace_root.clone())?;
            save_user_llm_config(&cfg.llm.api_key, &cfg.llm.base_url, &cfg.llm.model, effort)?;
            if let Err(err) = client.send_command(AppClientCommand::ReloadLlmConfig {
                api_key: cfg.llm.api_key.clone(),
                base_url: cfg.llm.base_url.clone(),
                model: cfg.llm.model.clone(),
            }) {
                tracing::warn!("failed to reload LLM config after reasoning update: {err}");
            }
            show_local_notice(
                app,
                NoticeLevel::Info,
                format!("Reasoning effort set to `{effort}`. Default is medium."),
            );
            return Ok(false);
        }
        ParsedInput::LocalSkillInsert(name) => {
            let response = match client.request_skills_list_typed().await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        NoticeLevel::Error,
                        format!("Failed to load skills: {err}"),
                    );
                    return Ok(false);
                }
            };
            app.bottom_pane
                .set_available_skills(response.skills.clone());
            let matches = response
                .skills
                .into_iter()
                .filter(|skill| skill.name.eq_ignore_ascii_case(&name))
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [] => {
                    show_local_notice(
                        app,
                        NoticeLevel::Warn,
                        format!(
                            "Skill '{name}' was not found. Use /skills to inspect available skills."
                        ),
                    );
                }
                [skill] => {
                    if !app
                        .bottom_pane
                        .attach_skill(skill.name.clone(), skill.path.display().to_string())
                    {
                        show_local_notice(
                            app,
                            NoticeLevel::Warn,
                            "Close the active picker before inserting a skill.".to_string(),
                        );
                    } else {
                        show_local_notice(
                            app,
                            NoticeLevel::Info,
                            format!(
                                "Inserted skill '{}'. Add your task text and submit when ready.",
                                skill.name
                            ),
                        );
                    }
                }
                _ => {
                    show_local_notice(
                        app,
                        NoticeLevel::Warn,
                        format!(
                            "Skill name '{name}' is ambiguous. Use /skills and pick a more specific name."
                        ),
                    );
                }
            }
            return Ok(false);
        }
        ParsedInput::LocalSkillsOpen => {
            let response = match client.request_skills_list_typed().await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        NoticeLevel::Error,
                        format!("Failed to load skills: {err}"),
                    );
                    return Ok(false);
                }
            };
            app.bottom_pane
                .set_available_skills(response.skills.clone());
            let text = if response.skills.is_empty() {
                "No skills discovered.\n\nChecked default locations:\n- <workspace>/.cloudagent/skills/\n- ~/.cloudagent/skills/".to_string()
            } else {
                let mut lines = Vec::new();
                lines.push("Discovered skills:".to_string());
                for skill in response.skills {
                    let mode = match skill.invocation_mode {
                        agent_core::SkillInvocationMode::Implicit => "implicit",
                        agent_core::SkillInvocationMode::Explicit => "explicit",
                    };
                    let deps = if skill.dependencies.tools.is_empty() {
                        String::new()
                    } else {
                        format!(" deps: {}", skill.dependencies.tools.join(", "))
                    };
                    lines.push(format!(
                        "- `{}` [{}]{}: {} ({})",
                        skill.name,
                        mode,
                        deps,
                        skill.description,
                        skill.path.display()
                    ));
                }
                lines.join("\n")
            };
            app.push_live_cell(HistoryCell::agent("skills", text, HistoryFormat::Markdown));
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
                agent_core::host::timestamp_conversation_id()
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
        ServerAction::InvalidateSkillsCatalog => {
            app.run_state.pending_skills_refresh = true;
            app.run_state.next_skills_refresh_at = None;
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
            app.run_state.history_has_more = false;
            app.run_state.history_next_before_turn_id = None;
            conversation_facade::rebuild_transcript_from_history(app);
        }
        ServerAction::ReplaceHistoryPage {
            turns,
            has_more,
            next_before_turn_id,
        } => {
            app.run_state.history_snapshot = Some(turns);
            app.run_state.history_has_more = has_more;
            app.run_state.history_next_before_turn_id = next_before_turn_id;
            conversation_facade::rebuild_transcript_from_history(app);
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
        ServerAction::StartActiveTurnItem {
            turn_id,
            item_id,
            kind,
            title,
        } => {
            app.on_server_active_item_started(&item_id, &kind, title.as_deref());
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
        ServerAction::AppendCommandOutputDelta { item_id, delta } => {
            app.bottom_pane
                .on_command_output_delta(Some(&item_id), &delta);
        }
        ServerAction::CompleteActiveTurnItem {
            turn_id,
            item_id,
            item,
        } => {
            if let agent_core::conversation::TranscriptItem::CommandExecution { status, .. } = &item
                && !matches!(status, agent_core::CommandExecutionStatus::InProgress)
            {
                app.bottom_pane.on_command_finished(&item_id);
            }
            app.transcript_owner.complete_item(
                turn_id,
                item_id,
                item,
                app.run_state.expand_tool_details,
            );
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

fn prepend_turn_page(
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

fn persist_cli_settings(app: &TuiApp) -> Result<()> {
    save_cli_settings(
        &app.conversation_store_dir,
        &PersistedCliSettings::new(
            app.run_state.pre_llm_filter_enabled,
            app.run_state.permission_mode.clone(),
        ),
    )
}

fn save_user_llm_config_default(api_key: &str, base_url: &str, model: &str) -> Result<()> {
    save_user_llm_config(api_key, base_url, model, ReasoningEffort::Medium)
}

fn save_user_llm_config(
    api_key: &str,
    base_url: &str,
    model: &str,
    reasoning_effort: ReasoningEffort,
) -> Result<()> {
    let path = reasoning_effort_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = toml::to_string(&UserConfigFile {
        llm: UserLlmConfig {
            api_key,
            base_url,
            model,
            model_reasoning_effort: reasoning_effort,
        },
    })?;
    fs::write(path, body)?;
    Ok(())
}

#[derive(Serialize)]
struct UserConfigFile<'a> {
    llm: UserLlmConfig<'a>,
}

#[derive(Serialize)]
struct UserLlmConfig<'a> {
    api_key: &'a str,
    base_url: &'a str,
    model: &'a str,
    model_reasoning_effort: ReasoningEffort,
}
fn reasoning_effort_config_path() -> Result<std::path::PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("Cannot find user home directory"))?;
    Ok(std::path::PathBuf::from(home)
        .join(".cloudagent")
        .join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::prepend_turn_page;
    use agent_core::{ConversationTurn, TurnState};

    fn turn(id: &str) -> ConversationTurn {
        ConversationTurn {
            id: id.to_string(),
            state: TurnState::Completed,
            items: Vec::new(),
            rollout_start_index: 0,
            rollout_end_index: 0,
        }
    }

    #[test]
    fn prepend_turn_page_keeps_old_to_new_order_and_deduplicates_boundary() {
        let merged = prepend_turn_page(
            vec![turn("turn-1"), turn("turn-2")],
            vec![turn("turn-2"), turn("turn-3")],
        );
        let ids = merged.into_iter().map(|turn| turn.id).collect::<Vec<_>>();

        assert_eq!(ids, vec!["turn-1", "turn-2", "turn-3"]);
    }
}
