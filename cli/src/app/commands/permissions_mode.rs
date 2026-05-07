use crate::app::TuiApp;
use crate::app::commands::permission_profile::{
    canonical_permission_mode, is_valid_permission_mode, permission_mode_label,
};

pub(crate) fn apply_permission_mode(app: &mut TuiApp, mode: &str) -> Result<(), &'static str> {
    if !is_valid_permission_mode(mode) {
        return Err("Invalid permission mode. Use /permissions and choose a mode.");
    }
    let canonical = canonical_permission_mode(mode);
    app.run_state.permission_mode = canonical.to_string();
    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::info(
        "context",
        format!(
            "Project permission mode set to `{canonical}` ({}).",
            permission_mode_label(canonical)
        ),
        crate::ui::widgets::history_cell::HistoryTone::Control,
    ));
    Ok(())
}
