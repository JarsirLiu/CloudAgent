use agent_protocol::PlatformControlEntry;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const PLATFORM_CONTROL_VERSION: u32 = 1;
const SUPPORTED_PLATFORMS: &[&str] = &["feishu", "wecom", "weixin"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PlatformControlState {
    pub(crate) version: u32,
    pub(crate) platforms: BTreeMap<String, PlatformControlEntry>,
}

pub(crate) fn supported_platforms() -> &'static [&'static str] {
    SUPPORTED_PLATFORMS
}

pub(crate) fn ensure_supported_platform(platform: &str) -> Result<()> {
    if supported_platforms().contains(&platform) {
        return Ok(());
    }
    bail!(
        "unsupported platform `{platform}`; supported platforms: {}",
        supported_platforms().join(", ")
    )
}

pub(crate) fn platform_control_path(data_root_dir: Option<&std::ffi::OsStr>) -> PathBuf {
    let root = data_root_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data"));
    root.join("platforms").join("control.json")
}

pub(crate) fn default_state() -> PlatformControlState {
    let mut platforms = BTreeMap::new();
    for platform in supported_platforms() {
        platforms.insert(platform.to_string(), default_entry(platform));
    }
    PlatformControlState {
        version: PLATFORM_CONTROL_VERSION,
        platforms,
    }
}

pub(crate) fn normalize_state(mut state: PlatformControlState) -> PlatformControlState {
    state.version = PLATFORM_CONTROL_VERSION;
    for platform in supported_platforms() {
        state
            .platforms
            .entry(platform.to_string())
            .or_insert_with(|| default_entry(platform));
    }
    state
}

pub(crate) fn default_entry(platform: &str) -> PlatformControlEntry {
    PlatformControlEntry {
        platform: platform.to_string(),
        enabled: false,
        configured: false,
        managed_by: "node".to_string(),
        updated_at_ms: 0,
    }
}

pub(crate) fn now_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| anyhow::anyhow!("system clock before unix epoch: {err}"))?
        .as_millis() as u64)
}
