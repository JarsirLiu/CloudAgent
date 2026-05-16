use crate::app::TuiApp;
use crate::app::conversation::actions::handle_tui_input;
use crate::app::runtime::display::{should_animate_live_status, should_animate_welcome};
use crate::state::reducer::UiInputEvent;
use agent_app_server_client::AppServerClient;
use std::time::{Duration, Instant};

const SKILL_REFRESH_RETRY_DELAY: Duration = Duration::from_secs(2);

pub(crate) fn pause_welcome_animation_for_input(app: &mut TuiApp) {
    app.welcome_animation_pause_ticks = 8;
}

pub(crate) async fn handle_animation_tick(app: &mut TuiApp, client: &AppServerClient) -> bool {
    let mut needs_redraw = false;
    if app.run_state.pending_skills_refresh
        && app
            .run_state
            .next_skills_refresh_at
            .is_none_or(|at| Instant::now() >= at)
    {
        match client.request_skills_list_typed().await {
            Ok(response) => {
                app.bottom_pane.set_available_skills(response.skills);
                app.run_state.pending_skills_refresh = false;
                app.run_state.next_skills_refresh_at = None;
                needs_redraw = true;
            }
            Err(err) => {
                tracing::warn!("failed to refresh skills catalog after invalidation: {err}");
                app.run_state.next_skills_refresh_at =
                    Some(Instant::now() + SKILL_REFRESH_RETRY_DELAY);
            }
        }
    }
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
    needs_redraw |= app.bottom_pane.handle_tick();
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
