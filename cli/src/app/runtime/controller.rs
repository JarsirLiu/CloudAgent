use crate::app::TuiApp;
use crate::app::conversation::actions::handle_tui_input;
use crate::app::conversation::event_router;
use crate::app::runtime::lifecycle::{handle_animation_tick, pause_welcome_animation_for_input};
use crate::terminal::{FrameRequester, UiEvent};
use agent_app_server_client::{AppServerClient, AppServerEvent};
use anyhow::Result;

pub(crate) struct RuntimeController;

impl RuntimeController {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn handle_client_event(
        &mut self,
        app: &mut TuiApp,
        event: AppServerEvent,
        frame_requester: &FrameRequester,
    ) {
        event_router::handle_client_event(app, event);
        frame_requester.schedule_frame();
    }

    pub(crate) fn handle_ui_event(
        &mut self,
        app: &mut TuiApp,
        client: &mut AppServerClient,
        event: UiEvent,
        frame_requester: &FrameRequester,
    ) -> Result<RuntimeControl> {
        let outcome = match event {
            UiEvent::Key(key) => {
                pause_welcome_animation_for_input(app);
                if let Some(input) = app.handle_key(key)
                    && handle_tui_input(app, client, input)?
                {
                    return Ok(RuntimeControl::Break);
                }
                frame_requester.schedule_frame();
                RuntimeControl::Continue
            }
            UiEvent::Paste(text) => {
                pause_welcome_animation_for_input(app);
                let _ = app.bottom_pane.handle_paste(&text);
                frame_requester.schedule_frame();
                RuntimeControl::Continue
            }
            UiEvent::Resize => {
                app.terminal_projection.request_history_replay();
                frame_requester.schedule_frame();
                RuntimeControl::Continue
            }
            UiEvent::Tick => {
                if handle_animation_tick(app) {
                    frame_requester.schedule_frame();
                }
                RuntimeControl::Continue
            }
            UiEvent::Draw => {
                frame_requester.finish_draw();
                RuntimeControl::Draw
            }
        };
        Ok(outcome)
    }
}

pub(crate) enum RuntimeControl {
    Continue,
    Draw,
    Break,
}
