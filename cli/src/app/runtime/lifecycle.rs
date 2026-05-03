use crate::app::TuiApp;

pub(crate) fn pause_welcome_animation_for_input(app: &mut TuiApp) {
    app.welcome_animation_pause_ticks = 8;
}

pub(crate) fn handle_animation_tick(app: &mut TuiApp) -> bool {
    app.run_state.clear_expired_notices();
    if app.welcome_animation_pause_ticks > 0 {
        app.welcome_animation_pause_ticks -= 1;
        return false;
    }
    if needs_animation_frame(app) {
        app.welcome_animation_frame = app.welcome_animation_frame.wrapping_add(1);
        return true;
    }
    false
}

fn needs_animation_frame(app: &TuiApp) -> bool {
    app.transcript_state.transcript.is_empty()
        && app.run_state.history_loaded
        && app.input_pane.composer_is_empty()
        && app.welcome_animation_pause_ticks == 0
}
