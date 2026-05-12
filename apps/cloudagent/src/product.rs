use agent_app_server_client::AppServerClient;
use agent_protocol::{
    NodeStatusResponse, NodeWorkerHealth, PlatformConfigResponse, PlatformControlEntry,
    PlatformControlListResponse, PlatformControlStatusResponse,
};
use anyhow::{Result, bail};
use cli::local_node::{connect_node_management_client, create_node_management_client};
use std::ffi::OsString;
use std::path::Path;

pub async fn maybe_handle_command(args: &[OsString], data_root_dir: &Path) -> Result<bool> {
    if maybe_handle_release_command(args, data_root_dir).await? {
        return Ok(true);
    }
    if maybe_handle_node_command(args, data_root_dir).await? {
        return Ok(true);
    }
    if maybe_handle_platform_command(args, data_root_dir).await? {
        return Ok(true);
    }
    Ok(false)
}

async fn maybe_handle_release_command(args: &[OsString], data_root_dir: &Path) -> Result<bool> {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return Ok(false);
    };

    match command {
        "start" => {
            let client = create_node_management_client(&args[1..], data_root_dir).await?;
            let response = client.request_node_status_typed().await?;
            println!("node ready");
            print_node_status_response(&response);
            Ok(true)
        }
        "status" => {
            let client = connect_node_management_client(&args[1..], data_root_dir).await?;
            print_node_status(&client).await?;
            Ok(true)
        }
        "stop" => {
            let client = connect_node_management_client(&args[1..], data_root_dir).await?;
            let response = client.stop_node_typed().await?;
            println!(
                "node stop requested: {}",
                if response.stopping {
                    "accepted"
                } else {
                    "ignored"
                }
            );
            Ok(true)
        }
        _ => Ok(false),
    }
}

async fn maybe_handle_node_command(args: &[OsString], data_root_dir: &Path) -> Result<bool> {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return Ok(false);
    };
    if command != "node" {
        return Ok(false);
    }
    let client = create_node_management_client(args, data_root_dir).await?;
    let action = args.get(1).and_then(|arg| arg.to_str()).unwrap_or("status");
    match action {
        "status" => print_node_status(&client).await?,
        "stop" => {
            let response = client.stop_node_typed().await?;
            println!(
                "node stop requested: {}",
                if response.stopping {
                    "accepted"
                } else {
                    "ignored"
                }
            );
        }
        other => bail!("unknown node action `{other}`. supported actions: status, stop"),
    }
    Ok(true)
}

async fn maybe_handle_platform_command(args: &[OsString], data_root_dir: &Path) -> Result<bool> {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return Ok(false);
    };
    if command != "platform" {
        return Ok(false);
    }
    let client = create_node_management_client(args, data_root_dir).await?;

    let action = args.get(1).and_then(|arg| arg.to_str()).unwrap_or("list");
    match action {
        "list" => {
            print_platform_list(&client).await?;
        }
        "status" => {
            if let Some(platform) = args.get(2).and_then(|arg| arg.to_str()) {
                print_platform_status(&client, platform).await?;
            } else {
                print_platform_list(&client).await?;
            }
        }
        "enable" => {
            let platform = args
                .get(2)
                .and_then(|arg| arg.to_str())
                .ok_or_else(|| anyhow::anyhow!("usage: cloudagent platform enable <platform>"))?;
            let response = client.set_platform_enabled_typed(platform, true).await?;
            println!("enabled platform `{platform}` via local node");
            print_single_platform(&response.platform);
        }
        "disable" => {
            let platform = args
                .get(2)
                .and_then(|arg| arg.to_str())
                .ok_or_else(|| anyhow::anyhow!("usage: cloudagent platform disable <platform>"))?;
            let response = client.set_platform_enabled_typed(platform, false).await?;
            println!("disabled platform `{platform}` via local node");
            print_single_platform(&response.platform);
        }
        "config" => {
            handle_platform_config_command(&client, args).await?;
        }
        other => bail!(
            "unknown platform action `{other}`. supported actions: list, status, enable, disable, config"
        ),
    }

    Ok(true)
}

async fn print_platform_list(client: &AppServerClient) -> Result<()> {
    let response: PlatformControlListResponse = client.request_platform_list_typed().await?;
    for entry in response.platforms {
        print_single_platform(&entry);
    }
    Ok(())
}

async fn print_platform_status(client: &AppServerClient, platform: &str) -> Result<()> {
    let response: PlatformControlStatusResponse =
        client.request_platform_status_typed(platform).await?;
    print_single_platform(&response.platform);
    Ok(())
}

fn print_single_platform(entry: &PlatformControlEntry) {
    let enabled = if entry.enabled { "enabled" } else { "disabled" };
    println!(
        "- {}: {enabled}, managed_by={}, updated_at_ms={}",
        entry.platform, entry.managed_by, entry.updated_at_ms
    );
}

async fn handle_platform_config_command(client: &AppServerClient, args: &[OsString]) -> Result<()> {
    let action = args.get(2).and_then(|arg| arg.to_str()).unwrap_or("get");
    match action {
        "get" => {
            let platform = args.get(3).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config get <platform>")
            })?;
            let response = client.request_platform_config_typed(platform).await?;
            print_platform_config(&response);
        }
        "set" => {
            let platform = args.get(3).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config set <platform> <key> <value>")
            })?;
            let key = args.get(4).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config set <platform> <key> <value>")
            })?;
            let value = args.get(5).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config set <platform> <key> <value>")
            })?;
            let response = client
                .set_platform_config_value_typed(platform, key, value)
                .await?;
            print_platform_config(&response);
        }
        "clear" => {
            let platform = args.get(3).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config clear <platform> <key>")
            })?;
            let key = args.get(4).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config clear <platform> <key>")
            })?;
            let response = client
                .clear_platform_config_value_typed(platform, key)
                .await?;
            print_platform_config(&response);
        }
        other => {
            bail!("unknown platform config action `{other}`. supported actions: get, set, clear")
        }
    }
    Ok(())
}

fn print_platform_config(response: &PlatformConfigResponse) {
    println!(
        "platform `{}`: {}",
        response.platform,
        if response.configured {
            "configured"
        } else {
            "incomplete"
        }
    );
    for field in &response.fields {
        let value = field.value.clone().unwrap_or_else(|| "<unset>".to_string());
        let required = if field.required {
            "required"
        } else {
            "optional"
        };
        let secret = if field.is_secret { ", secret" } else { "" };
        println!("- {}: {} ({required}{secret})", field.key, value);
    }
}

async fn print_node_status(client: &AppServerClient) -> Result<()> {
    let response: NodeStatusResponse = client.request_node_status_typed().await?;
    print_node_status_response(&response);
    Ok(())
}

fn print_node_status_response(response: &NodeStatusResponse) {
    println!("listen_address: {}", response.listen_address);
    println!(
        "worker: {}",
        if response.worker_running {
            "running"
        } else {
            "idle"
        }
    );
    println!(
        "platform_runtimes: {}/{}",
        response.platform_runtime_count, response.managed_platform_count
    );
    if !response.data_root_dir.is_empty() {
        println!("data_root_dir: {}", response.data_root_dir);
    }
    if !response.conversation_store_dir.is_empty() {
        println!(
            "conversation_store_dir: {}",
            response.conversation_store_dir
        );
    }
    if !response.workers.is_empty() {
        println!("worker_scopes:");
        for worker in &response.workers {
            match worker.health {
                NodeWorkerHealth::Running => {
                    println!(
                        "  {}: running (idle_for_ms={})",
                        worker.worker_scope_key,
                        worker.idle_for_ms.unwrap_or(0)
                    );
                }
                NodeWorkerHealth::Faulted => {
                    println!(
                        "  {}: faulted{}",
                        worker.worker_scope_key,
                        worker
                            .detail
                            .as_deref()
                            .map(|detail| format!(" ({detail})"))
                            .unwrap_or_default()
                    );
                }
            }
        }
    }
}
