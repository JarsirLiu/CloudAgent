use super::config_store::{
    PlatformConfigState, load_config_state, persist_config_state, platform_config_dir,
};
use super::control_store::{load_control_state, persist_control_state};
use super::runtime_control::{RunningPlatformRuntime, spawn_feishu_runtime, spawn_wecom_runtime};
use super::schema::{config_response, specs_for, validate_platform_config};
use super::state::{PlatformControlState, default_entry, ensure_supported_platform, now_ms};
use agent_protocol::{
    PlatformConfigResponse, PlatformControlListResponse, PlatformControlStatusResponse,
    PlatformControlUpdateResponse,
};
use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub(crate) struct PlatformManager {
    inner: Arc<Mutex<PlatformManagerState>>,
    data_root_dir: Option<std::ffi::OsString>,
    config_dir: PathBuf,
}

impl PlatformManager {
    pub(crate) async fn load(data_root_dir: Option<&std::ffi::OsStr>) -> Result<Self> {
        let config_dir = platform_config_dir(data_root_dir);
        let persisted = load_control_state(data_root_dir).await?;
        let config = load_config_state(&config_dir, super::state::supported_platforms()).await?;
        Ok(Self {
            inner: Arc::new(Mutex::new(PlatformManagerState {
                persisted,
                config,
                runtimes: BTreeMap::new(),
            })),
            data_root_dir: data_root_dir.map(|path| path.to_os_string()),
            config_dir,
        })
    }

    pub(crate) async fn list(&self) -> PlatformControlListResponse {
        let state = self.inner.lock().await;
        PlatformControlListResponse {
            platforms: state
                .persisted
                .platforms
                .values()
                .map(|entry| summarize_entry(entry, &state.config))
                .collect(),
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
        Ok(PlatformControlStatusResponse {
            platform: summarize_entry(&entry, &state.config),
        })
    }

    pub(crate) async fn config(&self, platform: &str) -> Result<PlatformConfigResponse> {
        ensure_supported_platform(platform)?;
        let state = self.inner.lock().await;
        Ok(config_response(&state.config, platform))
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
        let config = self.inner.lock().await.config.clone();
        Ok(PlatformControlUpdateResponse {
            platform: summarize_entry(&entry, &config),
        })
    }

    pub(crate) async fn set_config_value(
        &self,
        platform: &str,
        key: &str,
        value: &str,
        node_address: &str,
    ) -> Result<PlatformConfigResponse> {
        ensure_supported_platform(platform)?;
        ensure_supported_key(platform, key)?;
        {
            let mut state = self.inner.lock().await;
            state
                .config
                .platforms
                .entry(platform.to_string())
                .or_default()
                .insert(key.to_string(), value.to_string());
        }
        self.persist_config().await?;
        self.reload_if_running(platform, node_address).await?;
        self.config(platform).await
    }

    pub(crate) async fn clear_config_value(
        &self,
        platform: &str,
        key: &str,
        node_address: &str,
    ) -> Result<PlatformConfigResponse> {
        ensure_supported_platform(platform)?;
        ensure_supported_key(platform, key)?;
        {
            let mut state = self.inner.lock().await;
            if let Some(values) = state.config.platforms.get_mut(platform) {
                values.remove(key);
                if values.is_empty() {
                    state.config.platforms.remove(platform);
                }
            }
        }
        self.persist_config().await?;
        self.reload_if_running(platform, node_address).await?;
        self.config(platform).await
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

    pub(crate) async fn runtime_count(&self) -> usize {
        self.inner.lock().await.runtimes.len()
    }

    pub(crate) fn managed_platform_count(&self) -> usize {
        super::state::supported_platforms().len()
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
            let config = self.inner.lock().await.config.clone();
            let runtime =
                spawn_feishu_runtime(node_address, &config, self.data_root_dir.as_deref()).await?;
            self.ensure_runtime_started("feishu", runtime).await;
        } else {
            self.stop_runtime("feishu").await;
        }
        Ok(())
    }

    async fn apply_wecom(&self, enabled: bool, node_address: &str) -> Result<()> {
        if enabled {
            let config = self.inner.lock().await.config.clone();
            let runtime =
                spawn_wecom_runtime(node_address, &config, self.data_root_dir.as_deref()).await?;
            self.ensure_runtime_started("wecom", runtime).await;
        } else {
            self.stop_runtime("wecom").await;
        }
        Ok(())
    }

    async fn ensure_runtime_started(
        &self,
        platform: &str,
        runtime: super::runtime_control::SpawnedPlatformRuntime,
    ) {
        {
            let state = self.inner.lock().await;
            if state.runtimes.contains_key(platform) {
                runtime.into_running().abort();
                return;
            }
        }
        let mut state = self.inner.lock().await;
        if state.runtimes.contains_key(platform) {
            runtime.into_running().abort();
            return;
        }
        state
            .runtimes
            .insert(platform.to_string(), runtime.into_running());
    }

    async fn stop_runtime(&self, platform: &str) {
        if let Some(runtime) = self.inner.lock().await.runtimes.remove(platform) {
            runtime.abort();
        }
    }

    async fn persist(&self) -> Result<()> {
        let state = self.inner.lock().await.persisted.clone();
        persist_control_state(self.data_root_dir.as_deref(), &state).await
    }

    async fn persist_config(&self) -> Result<()> {
        let state = self.inner.lock().await.config.clone();
        persist_config_state(
            &self.config_dir,
            &state,
            super::state::supported_platforms(),
        )
        .await
    }

    async fn reload_if_running(&self, platform: &str, node_address: &str) -> Result<()> {
        let should_restart = self.inner.lock().await.runtimes.contains_key(platform);
        if !should_restart {
            return Ok(());
        }
        self.stop_runtime(platform).await;
        let enabled = self
            .inner
            .lock()
            .await
            .persisted
            .platforms
            .get(platform)
            .map(|entry| entry.enabled)
            .unwrap_or(false);
        if enabled {
            self.apply_runtime_change(platform, true, node_address)
                .await?;
        }
        Ok(())
    }
}

fn summarize_entry(
    entry: &agent_protocol::PlatformControlEntry,
    config: &PlatformConfigState,
) -> agent_protocol::PlatformControlEntry {
    let mut summary = entry.clone();
    summary.configured = validate_platform_config(&summary.platform, config).is_ok();
    summary
}

struct PlatformManagerState {
    persisted: PlatformControlState,
    config: PlatformConfigState,
    runtimes: BTreeMap<String, RunningPlatformRuntime>,
}

fn ensure_supported_key(platform: &str, key: &str) -> Result<()> {
    if specs_for(platform).iter().any(|spec| spec.key == key) {
        return Ok(());
    }
    bail!("unsupported config key `{key}` for platform `{platform}`")
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
