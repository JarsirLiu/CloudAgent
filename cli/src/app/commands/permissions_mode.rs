use crate::app::TuiApp;
use std::fs;
use std::path::PathBuf;

pub(crate) fn load_permission_mode(workspace_root: &PathBuf, conversation_id: &str) -> String {
    let path = settings_path(workspace_root);
    let Ok(text) = fs::read_to_string(path) else {
        return "safe".to_string();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return "safe".to_string();
    };
    v.get("conversation_permission_mode")
        .and_then(|m| m.get(conversation_id))
        .and_then(|s| s.as_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "safe".to_string())
}

pub(crate) fn apply_permission_mode(app: &mut TuiApp, mode: &str) -> Result<(), &'static str> {
    if !matches!(mode, "safe" | "balanced" | "danger") {
        return Err("Invalid permission mode. Use /permissions and choose a mode.");
    }
    app.run_state.permission_mode = mode.to_string();
    let _ = save_permission_mode(&app.workspace_root, &app.conversation_id, mode);
    app.run_state.set_system_notice(
        format!("Session permission mode: {mode}"),
        Some(std::time::Duration::from_secs(4)),
    );
    Ok(())
}

fn settings_path(workspace_root: &PathBuf) -> PathBuf {
    workspace_root.join("data").join("ui-settings.json")
}

fn save_permission_mode(
    workspace_root: &PathBuf,
    conversation_id: &str,
    mode: &str,
) -> std::io::Result<()> {
    let path = settings_path(workspace_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut root = if let Ok(text) = fs::read_to_string(&path) {
        serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if root.get("conversation_permission_mode").is_none() {
        root["conversation_permission_mode"] = serde_json::json!({});
    }
    root["conversation_permission_mode"][conversation_id] = serde_json::json!(mode);
    fs::write(path, serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string()))
}
