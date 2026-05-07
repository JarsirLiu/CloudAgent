use crate::app::TuiApp;
pub(crate) fn should_animate_live_status(app: &TuiApp) -> bool {
    app.current_mode() != agent_protocol::FrontendMode::Idle
}

pub(crate) fn should_animate_welcome(app: &TuiApp) -> bool {
    app.transcript_owner.live_is_empty()
        && !app.transcript_owner.has_transcript_content()
        && app.current_mode() == agent_protocol::FrontendMode::Idle
        && app.welcome_animation_pause_ticks == 0
}

pub(crate) fn should_show_welcome(app: &TuiApp) -> bool {
    !app.transcript_owner.has_transcript_content()
        && app.current_mode() == agent_protocol::FrontendMode::Idle
}
