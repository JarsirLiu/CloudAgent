use crate::app::TuiApp;
use crate::app::commands::permission_profile::{is_valid_permission_mode, permission_mode_label};

pub(crate) fn apply_permission_mode(app: &mut TuiApp, mode: &str) -> Result<(), &'static str> {
    if !is_valid_permission_mode(mode) {
        return Err("Invalid permission mode. Use /permissions and choose a mode.");
    }
    app.run_state.permission_mode = mode.to_string();
    app.run_state.set_system_notice(
        format!("Session permission mode: {mode} ({})", permission_mode_label(mode)),
        Some(std::time::Duration::from_secs(4)),
    );
    Ok(())
}
