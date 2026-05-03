use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedCliSettings {
    pub pre_llm_filter_enabled: bool,
    pub permission_mode: String,
}

impl PersistedCliSettings {
    pub fn new(pre_llm_filter_enabled: bool, permission_mode: String) -> Self {
        Self {
            pre_llm_filter_enabled,
            permission_mode,
        }
    }
}

pub fn load_cli_settings(store_root: &Path) -> Result<Option<PersistedCliSettings>> {
    let Some(snapshot) = storage::load_project_settings_snapshot_sync(store_root)? else {
        return Ok(None);
    };
    let parsed = serde_json::from_str::<PersistedCliSettings>(&snapshot)?;
    Ok(Some(parsed))
}

pub fn save_cli_settings(
    store_root: &Path,
    settings: &PersistedCliSettings,
) -> Result<()> {
    let payload = serde_json::to_string(settings)?;
    storage::save_project_settings_snapshot_sync(store_root, &payload)
}
