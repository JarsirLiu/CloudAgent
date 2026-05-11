use crate::node::data_root::resolve_data_root_dir;
use agent_core::{ApprovalPolicy, PermissionProfile};
use agent_protocol::TurnPolicy;
use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PersistedDeviceSettings {
    pub(crate) permission_mode: String,
}

impl PersistedDeviceSettings {
    pub(crate) fn default() -> Self {
        Self {
            permission_mode: "ReadOnly".to_string(),
        }
    }

    pub(crate) fn turn_policy(&self) -> TurnPolicy {
        TurnPolicy {
            permission_profile: permission_profile_for_mode(&self.permission_mode),
            approval_policy: ApprovalPolicy::OnRequest,
        }
    }
}

pub(crate) fn conversation_store_dir(data_root_dir: Option<&std::ffi::OsStr>) -> PathBuf {
    let root = resolve_data_root_dir(data_root_dir);
    root.join("conversations")
}

pub(crate) fn load_persisted_device_settings(store_root: &Path) -> Result<PersistedDeviceSettings> {
    let Some(snapshot) = infra_store::load_project_settings_snapshot_sync(store_root)? else {
        return Ok(PersistedDeviceSettings::default());
    };
    Ok(serde_json::from_str::<PersistedDeviceSettings>(&snapshot)
        .unwrap_or_else(|_| PersistedDeviceSettings::default()))
}

fn permission_profile_for_mode(mode: &str) -> PermissionProfile {
    match mode.trim().to_ascii_lowercase().as_str() {
        "readonly" | "safe" => PermissionProfile::ReadOnly,
        "workspacewrite" | "balanced" => PermissionProfile::WorkspaceWrite,
        "fullaccess" | "danger" => PermissionProfile::FullAccess,
        _ => PermissionProfile::ReadOnly,
    }
}

#[cfg(test)]
mod tests {
    use super::{PersistedDeviceSettings, conversation_store_dir, load_persisted_device_settings};
    use agent_core::PermissionProfile;

    #[test]
    fn conversation_store_dir_defaults_under_data_root() {
        assert_eq!(
            conversation_store_dir(None),
            std::path::PathBuf::from("data").join("conversations")
        );
    }

    #[test]
    fn permission_mode_maps_to_turn_policy() {
        let settings = PersistedDeviceSettings {
            permission_mode: "WorkspaceWrite".to_string(),
        };

        let policy = settings.turn_policy();
        assert!(matches!(
            policy.permission_profile,
            PermissionProfile::WorkspaceWrite
        ));
    }

    #[test]
    fn missing_snapshot_uses_defaults() {
        let temp = tempfile::tempdir().expect("tempdir");
        let settings = load_persisted_device_settings(temp.path()).expect("load default settings");
        assert_eq!(settings.permission_mode, "ReadOnly");
    }
}
