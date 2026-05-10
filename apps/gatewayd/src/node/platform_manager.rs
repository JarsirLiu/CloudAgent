use agent_app_server_client::{AppServerClient, AppServerConnectInfo, RemoteClientConfig};
use agent_core::{ApprovalPolicy, PermissionProfile};
use agent_gateway::adapter::{feishu, wecom};
use agent_protocol::{
    PlatformControlEntry, PlatformControlListResponse, PlatformControlStatusResponse,
    PlatformControlUpdateResponse, TurnPolicy,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

const PLATFORM_CONTROL_VERSION: u32 = 1;

#[derive(Clone)]
pub(crate) struct PlatformManager {
    inner: Arc<Mutex<PlatformManagerState>>,
    path: PathBuf,
}

impl PlatformManager {
    pub(crate) async fn load(data_root_dir: Option<&std::ffi::OsStr>) -> Result<Self> {
        let path = platform_control_path(data_root_dir);
        let state = if path.exists() {
            let text = tokio::fs::read_to_string(&path).await?;
            normalize_state(serde_json::from_str(&text)?)
        } else {
            default_state()
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(PlatformManagerState {
                persisted: state,
                runtimes: BTreeMap::new(),
            })),
            path,
        })
    }

    pub(crate) async fn list(&self) -> PlatformControlListResponse {
        let state = self.inner.lock().await;
        PlatformControlListResponse {
            platforms: state.persisted.platforms.values().cloned().collect(),
        }
    }

    pub(crate) async fn status(&self, platform: &str) -> Result<PlatformControlStatusResponse> {
        ensure_supported_platform(platform)?;
        let state = self.inner.lock().await;
        let entry = state
            .persisted
            .platforms
            .get(platform)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing platform state for `{platform}`"))?;
        Ok(PlatformControlStatusResponse { platform: entry })
    }

    pub(crate) async fn set_enabled(
        &self,
        platform: &str,
        enabled: bool,
        node_address: &str,
    ) -> Result<PlatformControlUpdateResponse> {
        ensure_supported_platform(platform)?;
        let entry = {
            let mut state = self.inner.lock().await;
            let existing = state
                .persisted
                .platforms
                .entry(platform.to_string())
                .or_insert_with(|| default_entry(platform));
            existing.enabled = enabled;
            existing.updated_at_ms = now_ms()?;
            existing.managed_by = "node".to_string();
            existing.clone()
        };
        self.persist().await?;
        self.apply_runtime_change(platform, enabled, node_address)
            .await?;
        Ok(PlatformControlUpdateResponse { platform: entry })
    }

    pub(crate) async fn sync_desired_state(&self, node_address: &str) -> Result<()> {
        let entries = {
            let state = self.inner.lock().await;
            state
                .persisted
                .platforms
                .values()
                .cloned()
                .collect::<Vec<_>>()
        };
        for entry in entries {
            self.apply_runtime_change(&entry.platform, entry.enabled, node_address)
                .await?;
        }
        Ok(())
    }

    async fn apply_runtime_change(
        &self,
        platform: &str,
        enabled: bool,
        node_address: &str,
    ) -> Result<()> {
        match platform {
            "feishu" => self.apply_feishu(enabled, node_address).await,
            "wecom" => self.apply_wecom(enabled, node_address).await,
            "weixin" => Ok(()),
            other => bail!("unsupported platform `{other}`"),
        }
    }

    async fn apply_feishu(&self, enabled: bool, node_address: &str) -> Result<()> {
        if enabled {
            {
                let state = self.inner.lock().await;
                if state.runtimes.contains_key("feishu") {
                    return Ok(());
                }
            }
            let runtime = spawn_feishu_runtime(node_address).await?;
            let mut state = self.inner.lock().await;
            if state.runtimes.contains_key("feishu") {
                runtime.into_running().abort();
                return Ok(());
            }
            state
                .runtimes
                .insert("feishu".to_string(), runtime.into_running());
        } else if let Some(runtime) = self.inner.lock().await.runtimes.remove("feishu") {
            runtime.abort();
        }
        Ok(())
    }

    async fn apply_wecom(&self, enabled: bool, node_address: &str) -> Result<()> {
        if enabled {
            {
                let state = self.inner.lock().await;
                if state.runtimes.contains_key("wecom") {
                    return Ok(());
                }
            }
            let runtime = spawn_wecom_runtime(node_address).await?;
            let mut state = self.inner.lock().await;
            if state.runtimes.contains_key("wecom") {
                runtime.into_running().abort();
                return Ok(());
            }
            state
                .runtimes
                .insert("wecom".to_string(), runtime.into_running());
        } else if let Some(runtime) = self.inner.lock().await.runtimes.remove("wecom") {
            runtime.abort();
        }
        Ok(())
    }

    async fn persist(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let state = self.inner.lock().await.persisted.clone();
        tokio::fs::write(&self.path, serde_json::to_vec_pretty(&state)?).await?;
        Ok(())
    }
}

struct PlatformManagerState {
    persisted: PlatformControlState,
    runtimes: BTreeMap<String, RunningPlatformRuntime>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct PlatformControlState {
    version: u32,
    platforms: BTreeMap<String, PlatformControlEntry>,
}

struct RunningPlatformRuntime {
    runtime_task: JoinHandle<Result<()>>,
}

impl RunningPlatformRuntime {
    fn abort(self) {
        self.runtime_task.abort();
    }
}

struct SpawnedPlatformRuntime {
    runtime_task: JoinHandle<Result<()>>,
}

impl SpawnedPlatformRuntime {
    fn into_running(self) -> RunningPlatformRuntime {
        RunningPlatformRuntime {
            runtime_task: self.runtime_task,
        }
    }
}

fn default_state() -> PlatformControlState {
    let mut platforms = BTreeMap::new();
    for platform in supported_platforms() {
        platforms.insert(platform.to_string(), default_entry(platform));
    }
    PlatformControlState {
        version: PLATFORM_CONTROL_VERSION,
        platforms,
    }
}

fn normalize_state(mut state: PlatformControlState) -> PlatformControlState {
    state.version = PLATFORM_CONTROL_VERSION;
    for platform in supported_platforms() {
        state
            .platforms
            .entry(platform.to_string())
            .or_insert_with(|| default_entry(platform));
    }
    state
}

fn default_entry(platform: &str) -> PlatformControlEntry {
    PlatformControlEntry {
        platform: platform.to_string(),
        enabled: false,
        managed_by: "node".to_string(),
        updated_at_ms: 0,
    }
}

fn supported_platforms() -> &'static [&'static str] {
    &["feishu", "wecom", "weixin"]
}

fn ensure_supported_platform(platform: &str) -> Result<()> {
    if supported_platforms().contains(&platform) {
        return Ok(());
    }
    bail!(
        "unsupported platform `{platform}`; supported platforms: {}",
        supported_platforms().join(", ")
    )
}

fn platform_control_path(data_root_dir: Option<&std::ffi::OsStr>) -> PathBuf {
    let root = data_root_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data"));
    root.join("platforms").join("control.json")
}

fn now_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| anyhow::anyhow!("system clock before unix epoch: {err}"))?
        .as_millis() as u64)
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

async fn spawn_feishu_runtime(node_address: &str) -> Result<SpawnedPlatformRuntime> {
    let config = load_feishu_config()?;
    let client = connect_node_client(node_address).await?;
    let runtime = feishu::spawn_runtime(config, client, default_turn_policy())?;
    let runtime_task = tokio::spawn(async move {
        runtime.wait().await?;
        Ok(())
    });
    Ok(SpawnedPlatformRuntime { runtime_task })
}

async fn spawn_wecom_runtime(node_address: &str) -> Result<SpawnedPlatformRuntime> {
    let config = load_wecom_config()?;
    let client = connect_node_client(node_address).await?;
    let runtime = wecom::spawn_runtime(config, client, default_turn_policy())?;
    let runtime_task = tokio::spawn(async move {
        runtime.wait().await?;
        Ok(())
    });
    Ok(SpawnedPlatformRuntime { runtime_task })
}

fn load_feishu_config() -> Result<feishu::FeishuAdapterConfig> {
    let app_id =
        std::env::var("CLOUDAGENT_FEISHU_APP_ID").context("missing CLOUDAGENT_FEISHU_APP_ID")?;
    let app_secret = std::env::var("CLOUDAGENT_FEISHU_APP_SECRET")
        .context("missing CLOUDAGENT_FEISHU_APP_SECRET")?;
    let domain = std::env::var("CLOUDAGENT_FEISHU_DOMAIN")
        .unwrap_or_else(|_| "https://open.feishu.cn".to_string());
    Ok(feishu::FeishuAdapterConfig {
        app_id,
        app_secret,
        domain,
        ..Default::default()
    })
}

fn load_wecom_config() -> Result<wecom::WecomAdapterConfig> {
    let bot_id =
        std::env::var("CLOUDAGENT_WECOM_BOT_ID").context("missing CLOUDAGENT_WECOM_BOT_ID")?;
    let bot_secret = std::env::var("CLOUDAGENT_WECOM_BOT_SECRET")
        .context("missing CLOUDAGENT_WECOM_BOT_SECRET")?;
    Ok(wecom::WecomAdapterConfig { bot_id, bot_secret })
}

#[cfg(test)]
mod tests {
    use super::PlatformManager;

    #[tokio::test]
    async fn list_returns_default_platforms() {
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = PlatformManager::load(Some(temp.path().as_os_str()))
            .await
            .expect("load");
        let response = manager.list().await;
        assert!(
            response
                .platforms
                .iter()
                .any(|entry| entry.platform == "feishu")
        );
    }

    #[tokio::test]
    async fn set_enabled_persists_control_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let manager = PlatformManager::load(Some(temp.path().as_os_str()))
            .await
            .expect("load");
        let updated = manager
            .set_enabled("weixin", true, "127.0.0.1:47070")
            .await
            .expect("enable");
        assert!(updated.platform.enabled);

        let reloaded = PlatformManager::load(Some(temp.path().as_os_str()))
            .await
            .expect("reload");
        let status = reloaded.status("weixin").await.expect("status");
        assert!(status.platform.enabled);
    }
}
