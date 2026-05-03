use crate::app::TuiApp;
use crate::app::conversation::actions::handle_tui_input;
use crate::app::conversation::event_router;
use crate::app::runtime::lifecycle::{handle_animation_tick, pause_welcome_animation_for_input};
use crate::terminal::{ScrollbackSurface, TerminalGuard, UiEvent, spawn_tui_event_loop};
use crate::ui::chat_surface::ChatSurface;
use agent_app_server_client::AppServerClient;
use anyhow::Result;

pub(crate) async fn run_tui_event_loop(
    app: &mut TuiApp,
    client: &mut AppServerClient,
) -> Result<()> {
    let mut terminal = TerminalGuard::new()?;
    let mut surface = ScrollbackSurface::new();
    let mut events = spawn_tui_event_loop();
    let mut needs_redraw = true;

    loop {
        if needs_redraw {
            if app.take_pending_history_rebuild() {
                surface.replace_all(&mut terminal, app.history_cells())?;
                app.clear_pending_history_cells();
            } else {
                surface.reflow_if_width_changed(&mut terminal, app.history_cells())?;
            }
            let pending_history_lines =
                surface.pending_lines(&terminal, app.drain_pending_history_cells())?;
            let height = ChatSurface::desired_height(app, terminal.terminal.size()?.width).max(1);
            terminal.draw_with_history(height, pending_history_lines, |frame| app.render(frame))?;
        }

        let redraw_after_event = tokio::select! {
            Some(event) = client.next_event() => {
                event_router::handle_client_event(app, event);
                true
            }
            Some(event) = events.recv() => {
                match event {
                    UiEvent::Key(key) => {
                        pause_welcome_animation_for_input(app);
                        if let Some(input) = app.handle_key(key) {
                            if handle_tui_input(app, client, input)? {
                                break;
                            }
                        }
                        true
                    }
                    UiEvent::Paste(text) => {
                        pause_welcome_animation_for_input(app);
                        let _ = app.input_pane.handle_paste(&text);
                        true
                    }
                    UiEvent::Resize => true,
                    UiEvent::Tick => handle_animation_tick(app),
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
