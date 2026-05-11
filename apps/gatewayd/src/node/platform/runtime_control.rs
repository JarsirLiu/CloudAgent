use super::config_store::PlatformConfigState;
use super::schema::{build_feishu_config, build_wecom_config};
use crate::node::device_settings::{conversation_store_dir, load_persisted_device_settings};
use crate::node::source::platform_runtime_client_name;
use agent_app_server_client::{AppServerClient, AppServerConnectInfo, RemoteClientConfig};
use agent_gateway::adapter::{feishu, wecom};
use agent_protocol::TurnPolicy;
use anyhow::Result;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{error, info};

pub(super) struct RunningPlatformRuntime {
    runtime_task: JoinHandle<Result<()>>,
}

impl RunningPlatformRuntime {
    pub(super) fn abort(self) {
        self.runtime_task.abort();
    }
}

pub(super) struct SpawnedPlatformRuntime {
    runtime_task: JoinHandle<Result<()>>,
}

impl SpawnedPlatformRuntime {
    pub(super) fn into_running(self) -> RunningPlatformRuntime {
        RunningPlatformRuntime {
            runtime_task: self.runtime_task,
        }
    }
}

pub(super) async fn spawn_feishu_runtime(
    node_address: &str,
    state: &PlatformConfigState,
    data_root_dir: Option<&std::ffi::OsStr>,
) -> Result<SpawnedPlatformRuntime> {
    let config = build_feishu_config(state)?;
    let client_name = platform_runtime_client_name("feishu");
    let stream_client = connect_node_client(node_address, &client_name).await?;
    let control_client = connect_node_client(node_address, &client_name).await?;
    let runtime = feishu::spawn_runtime(
        config,
        stream_client,
        control_client,
        load_default_turn_policy(data_root_dir)?,
    )?;
    let runtime_task = tokio::spawn(async move {
        match runtime.wait().await {
            Ok(status) => info!("feishu runtime stopped: {status:?}"),
            Err(err) => {
                error!("feishu runtime failed: {err:#}");
                return Err(err);
            }
        }
        Ok(())
    });
    Ok(SpawnedPlatformRuntime { runtime_task })
}

pub(super) async fn spawn_wecom_runtime(
    node_address: &str,
    state: &PlatformConfigState,
    data_root_dir: Option<&std::ffi::OsStr>,
) -> Result<SpawnedPlatformRuntime> {
    let config = build_wecom_config(state)?;
    let client_name = platform_runtime_client_name("wecom");
    let client = connect_node_client(node_address, &client_name).await?;
    let runtime = wecom::spawn_runtime(config, client, load_default_turn_policy(data_root_dir)?)?;
    let runtime_task = tokio::spawn(async move {
        match runtime.wait().await {
            Ok(status) => info!("wecom runtime stopped: {status:?}"),
            Err(err) => {
                error!("wecom runtime failed: {err:#}");
                return Err(err);
            }
        }
        Ok(())
    });
    Ok(SpawnedPlatformRuntime { runtime_task })
}

fn load_default_turn_policy(data_root_dir: Option<&std::ffi::OsStr>) -> Result<TurnPolicy> {
    let store_root = conversation_store_dir(data_root_dir);
    Ok(load_persisted_device_settings(&store_root)?.turn_policy())
}

async fn connect_node_client(node_address: &str, client_name: &str) -> Result<AppServerClient> {
    AppServerClient::remote(RemoteClientConfig {
        address: node_address.to_string(),
        client: AppServerConnectInfo {
            client_name: client_name.to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            experimental_api: true,
            opt_out_notification_methods: Vec::new(),
            channel_capacity: agent_app_server_client::DEFAULT_EVENT_CHANNEL_CAPACITY,
        },
        connect_timeout: Duration::from_secs(2),
        initialize_timeout: Duration::from_secs(5),
    })
    .await
}
