use crate::app::TuiApp;
use crate::app::conversation::actions::handle_tui_input;
use crate::app::conversation::event_router;
use crate::app::runtime::lifecycle::{handle_animation_tick, pause_welcome_animation_for_input};
use crate::app::runtime::render_gate::{RenderGate, RenderGateIntent};
use crate::terminal::{FrameRequester, UiEvent};
use agent_app_server_client::{AppServerClient, AppServerEvent};
use anyhow::Result;

pub(crate) struct RuntimeController {
    render_gate: RenderGate,
}

impl RuntimeController {
    pub(crate) fn new() -> Self {
        Self {
            render_gate: RenderGate::default(),
        }
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
                self.render_gate.apply_intent(RenderGateIntent::AppInput);
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
                self.render_gate.apply_intent(RenderGateIntent::AppInput);
                pause_welcome_animation_for_input(app);
                let _ = app.input_pane.handle_paste(&text);
                frame_requester.schedule_frame();
                RuntimeControl::Continue
            }
            UiEvent::ScrollbackBrowse => {
                self.render_gate
                    .apply_intent(RenderGateIntent::TerminalScrollbackBrowse);
                RuntimeControl::Continue
            }
            UiEvent::Resize => {
                self.render_gate.apply_intent(RenderGateIntent::Resize);
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
                if self.render_gate.allows_draw() {
                    RuntimeControl::Draw
                } else {
                    RuntimeControl::Continue
                }
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
