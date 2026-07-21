use crate::app::TuiApp;
use crate::app::commands::filter_toggle::apply_filter_toggle;
use crate::app::commands::parse::ParsedInput;
use crate::app::commands::permissions_mode::apply_permission_mode;
use crate::app::config::cli_settings::{PersistedCliSettings, save_cli_settings};
use crate::app::config::llm_config::{UserLlmSettings, save_user_llm_settings};
use crate::app::conversation::actions::show_local_notice;
use crate::app::model_catalog::{
    ModelCatalogSnapshot, ModelCatalogSnapshotState, ModelCatalogSource,
};
use agent_app_server_client::AppServerClient;
use anyhow::Result;
use config::ReasoningEffort;

pub(crate) async fn handle_workspace_input(
    app: &mut TuiApp,
    client: &AppServerClient,
    input: ParsedInput,
) -> Result<bool> {
    match input {
        ParsedInput::LocalPermissionMode(mode) => {
            if mode.trim().is_empty() {
                let current = app.run_state.permission_mode.clone();
                app.bottom_pane.set_permissions_picker(&current);
                return Ok(false);
            }
            if let Err(err) = apply_permission_mode(app, mode.trim()) {
                show_local_notice(app, crate::state::NoticeLevel::Warn, err);
            } else if let Err(err) = persist_cli_settings(app) {
                show_local_notice(
                    app,
                    crate::state::NoticeLevel::Warn,
                    format!("failed to persist permission mode: {err}"),
                );
            }
            Ok(false)
        }
        ParsedInput::LocalConfig {
            api_key,
            base_url,
            model,
        } => {
            if api_key.is_empty() && base_url.is_empty() && model.is_empty() {
                let cfg = UserLlmSettings::load(&app.workspace_root)?;
                app.bottom_pane
                    .set_config_panel(cfg.api_key, cfg.base_url, cfg.model);
                return Ok(false);
            }
            if base_url.trim().is_empty() || model.trim().is_empty() {
                show_local_notice(
                    app,
                    crate::state::NoticeLevel::Warn,
                    "Base URL and Model cannot be empty.",
                );
                return Ok(false);
            }
            save_user_llm_settings(
                &app.workspace_root,
                &api_key,
                &base_url,
                &model,
                ReasoningEffort::Medium,
            )?;
            app.model_catalog.reset().await;
            app.model_catalog
                .spawn_prewarm(base_url.clone(), api_key.clone());
            if let Err(err) =
                client.send_command(agent_protocol::AppClientCommand::ReloadLlmConfig {
                    api_key: api_key.clone(),
                    base_url: base_url.clone(),
                    model: model.clone(),
                })
            {
                tracing::warn!("failed to reload LLM config after save: {err}");
            }
            show_local_notice(
                app,
                crate::state::NoticeLevel::Info,
                "Saved API Key / Base URL / Model to the active configuration file.",
            );
            Ok(false)
        }
        ParsedInput::LocalReasoning(effort) => {
            if effort.trim().is_empty() {
                let cfg = UserLlmSettings::load(&app.workspace_root)?;
                app.bottom_pane.set_reasoning_picker(cfg.reasoning_effort);
                return Ok(false);
            }
            let effort = match effort.trim().parse::<ReasoningEffort>() {
                Ok(effort) => effort,
                Err(_) => {
                    show_local_notice(
                        app,
                        crate::state::NoticeLevel::Warn,
                        "Reasoning effort must be one of: low, medium, high.",
                    );
                    return Ok(false);
                }
            };
            let cfg = UserLlmSettings::load(&app.workspace_root)?;
            save_user_llm_settings(
                &app.workspace_root,
                &cfg.api_key,
                &cfg.base_url,
                &cfg.model,
                effort,
            )?;
            if let Err(err) =
                client.send_command(agent_protocol::AppClientCommand::ReloadLlmConfig {
                    api_key: cfg.api_key.clone(),
                    base_url: cfg.base_url.clone(),
                    model: cfg.model.clone(),
                })
            {
                tracing::warn!("failed to reload LLM config after reasoning update: {err}");
            }
            show_local_notice(
                app,
                crate::state::NoticeLevel::Info,
                format!("Reasoning effort set to `{effort}`. Default is medium."),
            );
            Ok(false)
        }
        ParsedInput::LocalModel(model) => {
            let trimmed = model.trim();
            if trimmed.is_empty() {
                let cfg = UserLlmSettings::load(&app.workspace_root)?;
                match app
                    .model_catalog
                    .load_for_picker(cfg.base_url.clone(), cfg.api_key.clone())
                    .await
                {
                    ModelCatalogSnapshotState::Ready(snapshot) => {
                        app.bottom_pane
                            .set_model_picker(cfg.model.clone(), snapshot.catalog.models.clone());
                        show_local_notice(
                            app,
                            crate::state::NoticeLevel::Info,
                            format_model_catalog_notice(&snapshot),
                        );
                    }
                    ModelCatalogSnapshotState::Loading | ModelCatalogSnapshotState::Empty => {
                        app.bottom_pane.set_model_picker_loading(cfg.model.clone());
                        show_local_notice(
                            app,
                            crate::state::NoticeLevel::Info,
                            "Loading model list...",
                        );
                    }
                    ModelCatalogSnapshotState::Failed(err) => {
                        show_local_notice(
                            app,
                            crate::state::NoticeLevel::Error,
                            format!("Failed to load model list: {err}"),
                        );
                    }
                }
                return Ok(false);
            }

            let mut cfg = UserLlmSettings::load(&app.workspace_root)?;
            cfg.model = trimmed.to_string();
            cfg.save(&app.workspace_root)?;
            if let Err(err) =
                client.send_command(agent_protocol::AppClientCommand::ReloadLlmConfig {
                    api_key: cfg.api_key.clone(),
                    base_url: cfg.base_url.clone(),
                    model: cfg.model.clone(),
                })
            {
                tracing::warn!("failed to reload model after model update: {err}");
            }
            show_local_notice(
                app,
                crate::state::NoticeLevel::Info,
                format!("Model switched to `{}`.", cfg.model),
            );
            Ok(false)
        }
        ParsedInput::LocalFilterToggle(raw_args) => {
            if raw_args.trim().is_empty() {
                app.bottom_pane.set_filter_picker();
                return Ok(false);
            }
            if let Err(usage) = apply_filter_toggle(app, &raw_args) {
                show_local_notice(app, crate::state::NoticeLevel::Warn, usage);
                return Ok(false);
            } else if let Err(err) = persist_cli_settings(app) {
                show_local_notice(
                    app,
                    crate::state::NoticeLevel::Warn,
                    format!("failed to persist filter setting: {err}"),
                );
            }
            Ok(false)
        }
        _ => unreachable!("workspace input dispatcher received non-workspace input"),
    }
}

fn format_model_catalog_notice(snapshot: &ModelCatalogSnapshot) -> String {
    let source = match snapshot.source {
        ModelCatalogSource::Memory => "memory",
        ModelCatalogSource::FreshCache => "cache",
        ModelCatalogSource::StaleCache => "stale cache",
        ModelCatalogSource::Network => snapshot.catalog.source_url.as_str(),
    };
    format!(
        "Loaded {} models from {}",
        snapshot.catalog.models.len(),
        source
    )
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
