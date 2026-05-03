use crate::app::TuiApp;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use std::fs;
use std::path::PathBuf;

pub(crate) fn apply_filter_toggle(app: &mut TuiApp, raw_args: &str) -> Result<(), &'static str> {
    let arg = raw_args.trim().to_ascii_lowercase();
    match arg.as_str() {
        "on" => {
            app.run_state.pre_llm_filter_enabled = true;
            let _ = save_filter_enabled(&app.workspace_root, true);
            app.run_state.set_system_notice(
                "Pre-LLM input filter: enabled",
                Some(std::time::Duration::from_secs(4)),
            );
            app.push_cell(HistoryCell::from_message(
                "context",
                "Pre-LLM input filter enabled for this local session.",
                HistoryTone::Control,
            ));
            Ok(())
        }
        "off" => {
            app.run_state.pre_llm_filter_enabled = false;
            let _ = save_filter_enabled(&app.workspace_root, false);
            app.run_state.set_system_notice(
                "Pre-LLM input filter: disabled",
                Some(std::time::Duration::from_secs(4)),
            );
            app.push_cell(HistoryCell::from_message(
                "context",
                "Pre-LLM input filter disabled for this local session.",
                HistoryTone::Control,
            ));
            Ok(())
        }
        _ => Err("Usage: /filter <on|off>"),
    }
}

fn settings_path(workspace_root: &PathBuf) -> PathBuf {
    workspace_root.join("data").join("ui-settings.json")
}

pub(crate) fn load_filter_enabled(workspace_root: &PathBuf) -> bool {
    let path = settings_path(workspace_root);
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("pre_llm_filter_enabled")
                .and_then(|b| b.as_bool())
        })
        .unwrap_or(false)
}

fn save_filter_enabled(workspace_root: &PathBuf, enabled: bool) -> std::io::Result<()> {
    let path = settings_path(workspace_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&serde_json::json!({
        "pre_llm_filter_enabled": enabled
    }))
    .unwrap_or_else(|_| "{\"pre_llm_filter_enabled\":false}".to_string());
    fs::write(path, text)
}
