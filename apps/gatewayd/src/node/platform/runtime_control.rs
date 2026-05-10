use super::config_store::PlatformConfigState;
use super::schema::{build_feishu_config, build_wecom_config};
use agent_app_server_client::{AppServerClient, AppServerConnectInfo, RemoteClientConfig};
use agent_core::{ApprovalPolicy, PermissionProfile};
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
) -> Result<SpawnedPlatformRuntime> {
    let config = build_feishu_config(state)?;
    let client = connect_node_client(node_address).await?;
    let runtime = feishu::spawn_runtime(config, client, default_turn_policy())?;
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
) -> Result<SpawnedPlatformRuntime> {
    let config = build_wecom_config(state)?;
    let client = connect_node_client(node_address).await?;
    let runtime = wecom::spawn_runtime(config, client, default_turn_policy())?;
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

fn default_turn_policy() -> TurnPolicy {
    TurnPolicy {
        permission_profile: PermissionProfile::ReadOnly,
        approval_policy: ApprovalPolicy::OnRequest,
    }
}

async fn connect_node_client(node_address: &str) -> Result<AppServerClient> {
    AppServerClient::remote(RemoteClientConfig {
        address: node_address.to_string(),
        client: AppServerConnectInfo {
            client_name: "gatewayd-platform".to_string(),
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
