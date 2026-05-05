use crate::app::TuiApp;

pub(crate) fn pause_welcome_animation_for_input(app: &mut TuiApp) {
    app.welcome_animation_pause_ticks = 8;
}

pub(crate) fn handle_animation_tick(app: &mut TuiApp) -> bool {
    app.run_state.clear_expired_notices();
    let mut needs_redraw = app.input_pane.handle_tick();
    if !matches!(
        app.runtime_projection.phase,
        None | Some(crate::state::runtime_projection::RuntimePhase::Idle)
    ) {
        app.run_state.live_animation_frame = app.run_state.live_animation_frame.wrapping_add(1);
        needs_redraw = true;
    }
    if app.welcome_animation_pause_ticks > 0 {
        app.welcome_animation_pause_ticks -= 1;
        return needs_redraw;
    }
    if needs_animation_frame(app) {
        app.welcome_animation_frame = app.welcome_animation_frame.wrapping_add(1);
        return true;
    }
    needs_redraw
}

fn needs_animation_frame(app: &TuiApp) -> bool {
    app.transcript_state.transcript.is_empty()
        && app.run_state.history_loaded
        && app.input_pane.composer_is_empty()
        && app.welcome_animation_pause_ticks == 0
}
