use crate::app::TuiApp;
use crate::state::runtime_projection::RuntimePhase;
pub(crate) fn should_animate_live_status(app: &TuiApp) -> bool {
    !matches!(
        app.runtime_projection.phase,
        None | Some(RuntimePhase::Idle)
    )
}

pub(crate) fn should_animate_welcome(app: &TuiApp) -> bool {
    app.transcript_owner.live_is_empty()
        && app.run_state.history_loaded
        && app.input_pane.composer_is_empty()
        && app.welcome_animation_pause_ticks == 0
}

pub(crate) fn should_show_welcome(app: &TuiApp) -> bool {
    !app.transcript_owner.has_transcript_content()
        && app.run_state.history_loaded
        && matches!(
            app.runtime_projection.phase,
            None | Some(RuntimePhase::Idle)
        )
}
