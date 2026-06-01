use crate::local_node::{arg_value, build_local_node_bootstrap};
use crate::{AppServerTarget, ConsoleBootstrap, ConsoleConfig, run_console};
use agent_core::host::timestamp_conversation_id;
use anyhow::{Result, bail};
use config::AgentConfig;
use std::ffi::OsString;
use std::path::PathBuf;

pub enum RequestedConsoleTarget {
    Public(AppServerTarget),
    Embedded,
}

pub fn apply_data_dir_cli_override(config: &mut AgentConfig, args: &[OsString]) {
    if let Some(value) = arg_value(args, "--data-dir") {
        let value = PathBuf::from(value);
        config.runtime.data_root_dir = if value.is_absolute() {
            value
        } else {
            config.workspace_root.join(value)
        };
        config.runtime.conversation_store_dir = config.runtime.data_root_dir.join("conversations");
        config.runtime.memory.root_dir = config.runtime.data_root_dir.join("state").join("memory");
    }
}

pub fn requested_console_target(
    args: &[OsString],
    allow_internal_targets: bool,
) -> Result<RequestedConsoleTarget> {
    let target = requested_target_name(args);

    match target.as_str() {
        "local-node" => Ok(RequestedConsoleTarget::Public(AppServerTarget::LocalNode)),
        "hub-node" => {
            let node_id = arg_value(args, "--hub-node-id")
                .or_else(|| std::env::var_os("CLOUDAGENT_HUB_NODE_ID"))
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| anyhow::anyhow!("hub-node target requires --hub-node-id"))?;
            Ok(RequestedConsoleTarget::Public(AppServerTarget::HubNode {
                node_id,
            }))
        }
        "embedded" if allow_internal_targets => Ok(RequestedConsoleTarget::Embedded),
        "embedded" => {
            bail!("target 'embedded' is internal-only and not part of the supported user path")
        }
        other => bail!("unknown target '{other}'. supported targets: local-node, hub-node"),
    }
}

pub async fn resolve_initial_conversation_id(args: &[OsString]) -> Result<String> {
    if let Some(conversation_id) =
        arg_value(args, "--conversation").and_then(|v| v.into_string().ok())
    {
        return Ok(conversation_id);
    }

    Ok(timestamp_conversation_id())
}

pub fn internal_targets_enabled() -> bool {
    std::env::var("CLOUDAGENT_INTERNAL_TARGETS").ok().as_deref() == Some("1")
}

pub fn resolve_console_target(
    args: &[OsString],
    runtime: Option<&std::sync::Arc<agent_core::AgentHost>>,
    requested_target: RequestedConsoleTarget,
    data_root_dir: &std::path::Path,
) -> Result<(String, ConsoleBootstrap)> {
    match requested_target {
        RequestedConsoleTarget::Public(AppServerTarget::LocalNode) => {
            Ok(build_local_node_bootstrap(args, data_root_dir))
        }
        RequestedConsoleTarget::Public(AppServerTarget::HubNode { node_id: _ }) => {
            bail!("target 'hub-node' is reserved for hub mode and is not implemented yet");
        }
        RequestedConsoleTarget::Embedded => Ok((
            "embedded".to_string(),
            ConsoleBootstrap::Embedded {
                runtime: runtime
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("embedded target requires a local runtime"))?,
            },
        )),
    }
}

pub async fn run_console_surface(
    args: &[OsString],
    config: AgentConfig,
    runtime_builder: impl FnOnce(AgentConfig) -> Result<std::sync::Arc<agent_core::AgentHost>>,
) -> Result<()> {
    let mut config = config;
    if let Ok(Some(settings)) =
        crate::app::cli_settings::load_cli_settings(&config.runtime.conversation_store_dir)
    {
        config.cli.pre_llm_filter_enabled = settings.pre_llm_filter_enabled;
        config.cli.permission_mode = settings.permission_mode;
    }
    let allow_internal_targets = internal_targets_enabled();
    let requested_target = requested_console_target(args, allow_internal_targets)?;
    let conversation_store_dir = config.runtime.conversation_store_dir.clone();
    let data_root_dir = config.runtime.data_root_dir.clone();
    let initial_filter_enabled = config.cli.pre_llm_filter_enabled;
    let initial_permission_mode = config.cli.permission_mode.clone();
    let conversation_history_turn_limit = config.cli.conversation_history_turn_limit;
    let conversation_id = resolve_initial_conversation_id(args).await?;
    let runtime = if matches!(requested_target, RequestedConsoleTarget::Embedded) {
        let runtime = runtime_builder(config)?;
        runtime.run_startup_retention_cleanup().await;
        Some(runtime)
    } else {
        None
    };
    let (target_label, bootstrap) =
        resolve_console_target(args, runtime.as_ref(), requested_target, &data_root_dir)?;

    run_console(ConsoleConfig {
        conversation_id,
        workspace_root: std::env::current_dir()?,
        conversation_store_dir,
        initial_filter_enabled,
        initial_permission_mode,
        conversation_history_turn_limit,
        auto_approve: false,
        auto_approve_reason: None,
        target_label,
        bootstrap,
    })
    .await
}

fn requested_target_name(args: &[OsString]) -> String {
    arg_value(args, "--target")
        .or_else(|| std::env::var_os("CLOUDAGENT_APP_SERVER_TARGET"))
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| "local-node".to_string())
}
