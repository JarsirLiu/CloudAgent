use crate::app::conversation::actions::platform_request_notice;
use crate::app::conversation::actions::show_local_notice;
use crate::app::TuiApp;
use crate::app::commands::parse::ParsedInput;
use crate::state::WeixinBindingState;
use crate::ui::widgets::gateway_panel::WeixinLoginSessionView;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryFormat};
use crate::ui::widgets::weixin_binding_view::WeixinBindingViewModel;
use agent_app_server_client::AppServerClient;
use anyhow::Result;

pub(crate) async fn handle_gateway_input(
    app: &mut TuiApp,
    client: &AppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::LocalGatewayOpen => {
            let response = match client.request_platform_list_typed().await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Error,
                        platform_request_notice("load platform list", &err),
                    );
                    return Ok(false);
                }
            };
            app.bottom_pane.set_gateway_list_panel(response.platforms);
            Ok(false)
        }
        ParsedInput::LocalGatewaySelect(platform) => {
            push_gateway_panel(app, client, &platform).await
        }
        ParsedInput::LocalGatewayWeixinLoginStart(platform) => {
            let response = match client.start_weixin_login_typed().await {
                Ok(response) => response,
                Err(err) => {
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Error,
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
                crate::state::NoticeLevel::Info,
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
                .push_weixin_binding_view(WeixinBindingViewModel {
                    platform,
                    session_id: response.session_id,
                    qr_url: response.qr_url,
                    status: "waiting for scan".to_string(),
                });
            Ok(false)
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
                        crate::state::NoticeLevel::Error,
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
                                crate::state::NoticeLevel::Error,
                                platform_request_notice("enable weixin connection", &err),
                            );
                            return Ok(false);
                        }
                    };
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Info,
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
                                crate::state::NoticeLevel::Error,
                                platform_request_notice("reload platform config", &err),
                            );
                            return Ok(false);
                        }
                    };
                    app.bottom_pane
                        .replace_parent_with_gateway_edit_panel(status.platform, config);
                    Ok(false)
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
                        .replace_weixin_binding_view(WeixinBindingViewModel {
                            platform,
                            session_id,
                            qr_url,
                            status: "waiting for confirmation".to_string(),
                        });
                    Ok(false)
                }
                "expired" => {
                    app.run_state.weixin_binding = None;
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Warn,
                        "Weixin QR expired. Start Connection again.",
                    );
                    reload_gateway_panel(app, client, &platform, None).await
                }
                _ => {
                    app.run_state.weixin_binding = None;
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Warn,
                        response
                            .message
                            .unwrap_or_else(|| "Weixin login session not found.".to_string()),
                    );
                    reload_gateway_panel(app, client, &platform, None).await
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
                        crate::state::NoticeLevel::Error,
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
                        crate::state::NoticeLevel::Error,
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
                        crate::state::NoticeLevel::Error,
                        platform_request_notice("reload platform config", &err),
                    );
                    return Ok(false);
                }
            };
            app.bottom_pane
                .replace_gateway_edit_panel(status.platform, config);
            show_local_notice(
                app,
                crate::state::NoticeLevel::Info,
                format!(
                    "Saved gateway settings for `{platform}`; connection is now {}",
                    enabled_label
                ),
            );
            Ok(false)
        }
        _ => unreachable!("gateway input dispatcher received non-gateway input"),
    }
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
                crate::state::NoticeLevel::Error,
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
                crate::state::NoticeLevel::Error,
                platform_request_notice("load platform config", &err),
            );
            return Ok(false);
        }
    };
    if platform == "weixin" {
        app.bottom_pane
            .replace_gateway_edit_panel_with_weixin_login(status.platform, config, weixin_login);
    } else {
        app.bottom_pane
            .replace_gateway_edit_panel(status.platform, config);
    }
    Ok(false)
}

async fn push_gateway_panel(
    app: &mut TuiApp,
    client: &AppServerClient,
    platform: &str,
) -> Result<bool> {
    let status = match client.request_platform_status_typed(platform).await {
        Ok(status) => status,
        Err(err) => {
            show_local_notice(
                app,
                crate::state::NoticeLevel::Error,
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
                crate::state::NoticeLevel::Error,
                platform_request_notice("load platform config", &err),
            );
            return Ok(false);
        }
    };
    app.bottom_pane
        .push_gateway_edit_panel(status.platform, config);
    Ok(false)
}
