use crate::app::TuiApp;
use crate::app::commands::permission_profile::{
    canonical_permission_mode, is_valid_permission_mode, permission_mode_label,
};
use crate::state::NoticeLevel;

pub(crate) fn apply_permission_mode(app: &mut TuiApp, mode: &str) -> Result<(), &'static str> {
    if !is_valid_permission_mode(mode) {
        return Err("Invalid permission mode. Use /permissions and choose a mode.");
    }
    let canonical = canonical_permission_mode(mode);
    app.run_state.permission_mode = canonical.to_string();
    app.bottom_pane.show_transient_notice(
        NoticeLevel::Info,
        format!(
            "Project permission mode set to `{canonical}` ({}).",
            permission_mode_label(canonical)
        ),
    );
    Ok(())
}
