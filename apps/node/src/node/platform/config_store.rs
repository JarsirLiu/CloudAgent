use crate::node::data_root::resolve_data_root_dir;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const PLATFORM_CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PlatformConfigState {
    pub(crate) version: u32,
    pub(crate) platforms: BTreeMap<String, BTreeMap<String, String>>,
}

pub(crate) async fn load_config_state(
    dir: &Path,
    platforms: &[&str],
) -> Result<PlatformConfigState> {
    let mut state = default_config_state();
    for platform in platforms {
        let path = platform_config_file(dir, platform);
        if !path.exists() {
            continue;
        }
        let text = tokio::fs::read_to_string(&path).await?;
        let values: BTreeMap<String, String> = serde_json::from_str(&text)?;
        if !values.is_empty() {
            state.platforms.insert((*platform).to_string(), values);
        }
    }
    Ok(state)
}

pub(crate) async fn persist_config_state(
    dir: &Path,
    state: &PlatformConfigState,
    platforms: &[&str],
) -> Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    for platform in platforms {
        let path = platform_config_file(dir, platform);
        match state.platforms.get(*platform) {
            Some(values) if !values.is_empty() => {
                tokio::fs::write(&path, serde_json::to_vec_pretty(values)?).await?;
            }
            _ if path.exists() => {
                tokio::fs::remove_file(&path).await?;
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn platform_config_dir(data_root_dir: Option<&std::ffi::OsStr>) -> PathBuf {
    let root = resolve_data_root_dir(data_root_dir);
    match (
        root.file_name().and_then(|name| name.to_str()),
        root.parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str()),
    ) {
        (Some("data"), Some(".cloudagent")) => root
            .parent()
            .map(|parent| parent.join("platform"))
            .unwrap_or_else(|| root.join("platform")),
        _ => root.join("platform"),
    }
}

fn default_config_state() -> PlatformConfigState {
    PlatformConfigState {
        version: PLATFORM_CONFIG_VERSION,
        platforms: BTreeMap::new(),
    }
}

fn platform_config_file(dir: &Path, platform: &str) -> PathBuf {
    dir.join(format!("{platform}.json"))
}

#[cfg(test)]
mod tests {
    use super::platform_config_dir;
    use std::path::PathBuf;

    #[test]
    fn dev_mode_uses_data_platform_directory() {
        let root = PathBuf::from(r"D:\repo\cloudagent\data");
        assert_eq!(
            platform_config_dir(Some(root.as_os_str())),
            PathBuf::from(r"D:\repo\cloudagent\data\platform")
        );
    }

    #[test]
    fn release_mode_uses_user_platform_directory() {
        let root = PathBuf::from(r"C:\Users\felix\.cloudagent\data");
        assert_eq!(
            platform_config_dir(Some(root.as_os_str())),
            PathBuf::from(r"C:\Users\felix\.cloudagent\platform")
        );
    }
}
