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
    let (mut events, frame_requester) = spawn_tui_event_loop();
    let mut controller = RuntimeController::new();
    frame_requester.schedule_frame();

    loop {
        let control = tokio::select! {
            Some(event) = client.next_event() => {
                controller.handle_client_event(app, event, &frame_requester);
                RuntimeControl::Continue
            }
            Some(event) = events.recv() => {
                controller.handle_ui_event(app, client, event, &frame_requester)?
            }
            else => break,
        };

        match control {
            RuntimeControl::Continue => {}
            RuntimeControl::Draw => draw_app_frame(app, &mut terminal)?,
            RuntimeControl::Break => break,
        }

        if app.run_state.should_exit {
            break;
        }
    }

    Ok(())
}
