use crate::app::TuiApp;
use crate::app::runtime::display::{should_animate_live_status, should_animate_welcome};

pub(crate) fn pause_welcome_animation_for_input(app: &mut TuiApp) {
    app.welcome_animation_pause_ticks = 8;
}

pub(crate) fn handle_animation_tick(app: &mut TuiApp) -> bool {
    app.run_state.clear_expired_notices();
    let mut needs_redraw = app.input_pane.handle_tick();
    if should_animate_live_status(app) {
        app.run_state.live_animation_frame = app.run_state.live_animation_frame.wrapping_add(1);
        needs_redraw = true;
    }
    if app.welcome_animation_pause_ticks > 0 {
        app.welcome_animation_pause_ticks -= 1;
        return needs_redraw;
    }
    if should_animate_welcome(app) {
        app.welcome_animation_frame = app.welcome_animation_frame.wrapping_add(1);
        return true;
    }
    needs_redraw
}
