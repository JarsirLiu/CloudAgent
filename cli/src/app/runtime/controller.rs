use crate::app::TuiApp;
use crate::app::conversation::actions::handle_tui_input;
use crate::app::conversation::event_router;
use crate::app::runtime::lifecycle::{handle_animation_tick, pause_welcome_animation_for_input};
use crate::app::runtime::render_gate::{RenderGate, RenderGateIntent};
use crate::terminal::UiEvent;
use agent_app_server_client::{AppServerClient, AppServerEvent};
use anyhow::Result;
use tokio::sync::mpsc;

pub(crate) struct RuntimeController {
    render_gate: RenderGate,
}

impl RuntimeController {
    pub(crate) fn new() -> Self {
        Self {
            render_gate: RenderGate::default(),
        }
    }

    pub(crate) fn should_draw(&self, needs_redraw: bool) -> bool {
        self.render_gate.should_draw(needs_redraw)
    }

    pub(crate) fn handle_client_event_batch(
        &mut self,
        app: &mut TuiApp,
        _client: &mut AppServerClient,
        first_event: AppServerEvent,
    ) -> bool {
        event_router::handle_client_event(app, first_event);
        true
    }

    pub(crate) fn handle_ui_event_batch(
        &mut self,
        app: &mut TuiApp,
        client: &mut AppServerClient,
        events: &mut mpsc::UnboundedReceiver<UiEvent>,
        first_event: UiEvent,
    ) -> Result<RuntimeControl> {
        let mut needs_redraw = self.handle_single_ui_event(app, client, first_event)?;
        if matches!(needs_redraw, RuntimeControl::Break) {
            return Ok(RuntimeControl::Break);
        }

        while let Ok(event) = events.try_recv() {
            match self.handle_single_ui_event(app, client, event)? {
                RuntimeControl::Continue(redraw) => {
                    needs_redraw = RuntimeControl::Continue(
                        matches!(needs_redraw, RuntimeControl::Continue(true)) || redraw,
                    );
                }
                RuntimeControl::Break => return Ok(RuntimeControl::Break),
            }
        }

        Ok(needs_redraw)
    }

    fn handle_single_ui_event(
        &mut self,
        app: &mut TuiApp,
        client: &mut AppServerClient,
        event: UiEvent,
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
                true
            }
            UiEvent::Paste(text) => {
                self.render_gate.apply_intent(RenderGateIntent::AppInput);
                pause_welcome_animation_for_input(app);
                let _ = app.input_pane.handle_paste(&text);
                true
            }
            UiEvent::ScrollbackBrowse => {
                self.render_gate
                    .apply_intent(RenderGateIntent::TerminalScrollbackBrowse);
                false
            }
            UiEvent::Resize => {
                self.render_gate.apply_intent(RenderGateIntent::Resize);
                true
            }
            UiEvent::Tick => handle_animation_tick(app),
        };
        Ok(RuntimeControl::Continue(outcome))
    }
}

pub(crate) enum RuntimeControl {
    Continue(bool),
    Break,
}
