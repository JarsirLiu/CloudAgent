use crate::app::TuiApp;
use crate::app::conversation::actions::handle_tui_input;
use crate::app::runtime::display::{should_animate_live_status, should_animate_welcome};
use agent_app_server_client::AppServerClient;
use crate::state::reducer::UiInputEvent;

pub(crate) fn pause_welcome_animation_for_input(app: &mut TuiApp) {
    app.welcome_animation_pause_ticks = 8;
}

pub(crate) async fn handle_animation_tick(
    app: &mut TuiApp,
    client: &AppServerClient,
) -> bool {
    if let Some(binding) = app.run_state.weixin_binding.clone()
        && std::time::Instant::now() >= binding.next_poll_at
    {
        let _ = handle_tui_input(
            app,
            client,
            UiInputEvent::LocalGatewayWeixinLoginCheck {
                platform: binding.platform,
                session_id: binding.session_id,
                qr_url: binding.qr_url,
            },
        )
        .await;
    }
    let mut needs_redraw = app.bottom_pane.handle_tick();
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
