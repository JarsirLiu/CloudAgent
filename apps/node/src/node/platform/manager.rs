use super::config_store::{
    PlatformConfigState, load_config_state, persist_config_state, platform_config_dir,
};
use super::control_store::{load_control_state, persist_control_state};
use super::runtime_control::{
    RunningPlatformRuntime, spawn_feishu_runtime, spawn_wecom_runtime, spawn_weixin_runtime,
};
use super::schema::{config_response, supported_specs_for, validate_platform_config};
use super::state::{PlatformControlState, default_entry, ensure_supported_platform, now_ms};
use agent_protocol::{
    PlatformConfigResponse, PlatformControlListResponse, PlatformControlStatusResponse,
    PlatformControlUpdateResponse, WeixinLoginStartResponse, WeixinLoginStatusResponse,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

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
                weixin_login_sessions: BTreeMap::new(),
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

    pub(crate) async fn start_weixin_login(&self) -> Result<WeixinLoginStartResponse> {
        let session_id = format!("weixin-login-{}", now_ms()?);
        let qr = request_weixin_qr().await?;
        let task = tokio::spawn(run_weixin_login_poll(
            qr.qrcode_value.clone(),
            qr.base_url.clone(),
        ));
        self.inner.lock().await.weixin_login_sessions.insert(
            session_id.clone(),
            WeixinLoginSession {
                qr_url: qr.qr_url.clone(),
                task,
            },
        );
        Ok(WeixinLoginStartResponse {
            session_id,
            qr_url: qr.qr_url,
        })
    }

    pub(crate) async fn check_weixin_login(
        &self,
        session_id: &str,
    ) -> Result<WeixinLoginStatusResponse> {
        let session = {
            let mut state = self.inner.lock().await;
            state.weixin_login_sessions.remove(session_id)
        };
        let Some(session) = session else {
            return Ok(WeixinLoginStatusResponse {
                session_id: session_id.to_string(),
                status: "missing".to_string(),
                account_id: None,
                message: Some("扫码会话不存在或已结束".to_string()),
            });
        };
        if !session.task.is_finished() {
            self.inner
                .lock()
                .await
                .weixin_login_sessions
                .insert(session_id.to_string(), session);
            return Ok(WeixinLoginStatusResponse {
                session_id: session_id.to_string(),
                status: "pending".to_string(),
                account_id: None,
                message: Some(session_id.to_string()),
            });
        }
        let result = session.task.await??;
        match result {
            WeixinLoginPollResult::Confirmed(credentials) => {
                {
                    let mut state = self.inner.lock().await;
                    let entry = state
                        .config
                        .platforms
                        .entry("weixin".to_string())
                        .or_default();
                    entry.insert("account_id".to_string(), credentials.account_id.clone());
                    entry.insert("token".to_string(), credentials.token.clone());
                    entry.insert("base_url".to_string(), credentials.base_url.clone());
                }
                self.persist_config().await?;
                Ok(WeixinLoginStatusResponse {
                    session_id: session_id.to_string(),
                    status: "confirmed".to_string(),
                    account_id: Some(credentials.account_id),
                    message: Some("微信凭据已写入本地配置".to_string()),
                })
            }
            WeixinLoginPollResult::Pending => {
                self.inner.lock().await.weixin_login_sessions.insert(
                    session_id.to_string(),
                    WeixinLoginSession {
                        qr_url: session.qr_url,
                        task: tokio::spawn(async { Ok(WeixinLoginPollResult::Pending) }),
                    },
                );
                Ok(WeixinLoginStatusResponse {
                    session_id: session_id.to_string(),
                    status: "pending".to_string(),
                    account_id: None,
                    message: Some("等待扫码确认".to_string()),
                })
            }
            WeixinLoginPollResult::Expired => Ok(WeixinLoginStatusResponse {
                session_id: session_id.to_string(),
                status: "expired".to_string(),
                account_id: None,
                message: Some("二维码已过期，请重新执行 /weixin-login".to_string()),
            }),
        }
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
            "weixin" => self.apply_weixin(enabled, node_address).await,
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

    async fn apply_weixin(&self, enabled: bool, node_address: &str) -> Result<()> {
        if enabled {
            let config = self.inner.lock().await.config.clone();
            let runtime =
                spawn_weixin_runtime(node_address, &config, self.data_root_dir.as_deref()).await?;
            self.ensure_runtime_started("weixin", runtime).await;
        } else {
            self.stop_runtime("weixin").await;
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
    weixin_login_sessions: BTreeMap<String, WeixinLoginSession>,
}

struct WeixinLoginSession {
    qr_url: String,
    task: JoinHandle<Result<WeixinLoginPollResult>>,
}

struct WeixinQrPayload {
    qrcode_value: String,
    qr_url: String,
    base_url: String,
}

struct WeixinCredentials {
    account_id: String,
    token: String,
    base_url: String,
}

enum WeixinLoginPollResult {
    Pending,
    Confirmed(WeixinCredentials),
    Expired,
}

async fn request_weixin_qr() -> Result<WeixinQrPayload> {
    let response =
        reqwest::get("https://ilinkai.weixin.qq.com/ilink/bot/get_bot_qrcode?bot_type=3")
            .await
            .context("failed to request weixin qr")?
            .error_for_status()
            .context("weixin qr returned error status")?
            .json::<Value>()
            .await
            .context("failed to decode weixin qr response")?;
    let qrcode_value = response
        .get("qrcode")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let qr_url = response
        .get("qrcode_img_content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if qrcode_value.is_empty() || qr_url.is_empty() {
        bail!("weixin qr response missing qrcode or qrcode_img_content")
    }
    Ok(WeixinQrPayload {
        qrcode_value,
        qr_url,
        base_url: "https://ilinkai.weixin.qq.com".to_string(),
    })
}

async fn run_weixin_login_poll(
    qrcode_value: String,
    mut current_base_url: String,
) -> Result<WeixinLoginPollResult> {
    let client = reqwest::Client::new();
    for _ in 0..120 {
        let url = format!(
            "{}/ilink/bot/get_qrcode_status?qrcode={}",
            current_base_url.trim_end_matches('/'),
            qrcode_value
        );
        let response = client
            .get(url)
            .send()
            .await
            .context("failed to poll weixin qr status")?
            .error_for_status()
            .context("weixin qr status returned error status")?
            .json::<Value>()
            .await
            .context("failed to decode weixin qr status")?;
        match response
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("wait")
        {
            "confirmed" => {
                let account_id = response
                    .get("ilink_bot_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let token = response
                    .get("bot_token")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let base_url = response
                    .get("baseurl")
                    .and_then(Value::as_str)
                    .unwrap_or("https://ilinkai.weixin.qq.com")
                    .to_string();
                if account_id.is_empty() || token.is_empty() {
                    bail!("weixin confirmed qr payload missing account_id or token")
                }
                return Ok(WeixinLoginPollResult::Confirmed(WeixinCredentials {
                    account_id,
                    token,
                    base_url,
                }));
            }
            "expired" => return Ok(WeixinLoginPollResult::Expired),
            "scaned_but_redirect" => {
                if let Some(host) = response.get("redirect_host").and_then(Value::as_str)
                    && !host.trim().is_empty()
                {
                    current_base_url = format!("https://{}", host.trim());
                }
            }
            _ => {}
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Ok(WeixinLoginPollResult::Pending)
}

fn ensure_supported_key(platform: &str, key: &str) -> Result<()> {
    if supported_specs_for(platform)
        .iter()
        .any(|spec| spec.key == key)
    {
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
        manager
            .set_config_value("weixin", "account_id", "acct_1", "127.0.0.1:47070")
            .await
            .expect("set account_id");
        manager
            .set_config_value("weixin", "token", "token_1", "127.0.0.1:47070")
            .await
            .expect("set token");
        let updated = manager
            .set_enabled("weixin", false, "127.0.0.1:47070")
            .await
            .expect("disable");
        assert!(!updated.platform.enabled);

        let reloaded = PlatformManager::load(Some(temp.path().as_os_str()))
            .await
            .expect("reload");
        let status = reloaded.status("weixin").await.expect("status");
        assert!(!status.platform.enabled);
    }
}
