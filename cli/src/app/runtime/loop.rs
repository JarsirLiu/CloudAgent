use crate::app::TuiApp;
use crate::app::runtime::controller::{RuntimeControl, RuntimeController};
use crate::app::runtime::render_pass::draw_app_frame;
use crate::terminal::{TerminalGuard, spawn_tui_event_loop};
use agent_app_server_client::AppServerClient;
use anyhow::Result;

pub(crate) async fn run_tui_event_loop(
    app: &mut TuiApp,
    client: &mut AppServerClient,
) -> Result<()> {
    let mut terminal = TerminalGuard::new()?;
    let mut events = spawn_tui_event_loop();
    let mut needs_redraw = true;
    let mut controller = RuntimeController::new();

    loop {
        if controller.should_draw(needs_redraw) {
            draw_app_frame(app, &mut terminal)?;
        }

        let redraw_after_event = tokio::select! {
            Some(event) = client.next_event() => {
                controller.handle_client_event_batch(app, client, event)
            }
            Some(event) = events.recv() => {
                match controller.handle_ui_event_batch(app, client, &mut events, event)? {
                    RuntimeControl::Continue(redraw) => redraw,
                    RuntimeControl::Break => break,
                }
            }
            else => break,
        };
        needs_redraw = redraw_after_event;

        if app.run_state.should_exit {
            break;
        }
    }

    Ok(())
}
