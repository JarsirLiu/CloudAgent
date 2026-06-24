use crate::app::TuiApp;
use crate::app::conversation::actions::handle_tui_input;
use crate::app::conversation::event_router;
use crate::app::input::clipboard_paste::paste_clipboard_text;
use crate::app::runtime::lifecycle::{handle_animation_tick, pause_welcome_animation_for_input};
use crate::app::runtime::paste_coordinator::PasteCoordinator;
use crate::terminal::{FrameRequester, UiEvent};
use agent_app_server_client::{AppServerClient, AppServerEvent};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

pub(crate) struct RuntimeController {
    paste_coordinator: PasteCoordinator,
}

impl RuntimeController {
    pub(crate) fn new() -> Self {
        Self {
            paste_coordinator: PasteCoordinator::default(),
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

    pub(crate) async fn handle_ui_event(
        &mut self,
        app: &mut TuiApp,
        client: &mut AppServerClient,
        event: UiEvent,
        frame_requester: &FrameRequester,
    ) -> Result<RuntimeControl> {
        let outcome = match event {
            UiEvent::Key(key) => {
                pause_welcome_animation_for_input(app);
                let supports_text_paste_shortcut = app.bottom_pane.supports_text_paste_shortcut();
                if self
                    .paste_coordinator
                    .should_handle_text_shortcut(key, supports_text_paste_shortcut)
                    && self.handle_text_paste_shortcut(app)
                {
                    frame_requester.schedule_frame();
                    RuntimeControl::Continue
                } else {
                    if should_request_older_history_page(key)
                        && app.transcript_scroll.is_at_top()
                        && crate::app::conversation::actions::load_older_history_page_if_available(
                            app, client,
                        )
                        .await?
                    {
                        frame_requester.schedule_frame();
                        return Ok(RuntimeControl::Continue);
                    }
                    if let Some(input) = app.handle_key(key)
                        && handle_tui_input(app, client, input).await?
                    {
                        return Ok(RuntimeControl::Break);
                    }
                    if let Some(delay) = app.bottom_pane.next_paste_flush_delay() {
                        frame_requester.schedule_tick_in(delay);
                    }
                    frame_requester.schedule_frame();
                    RuntimeControl::Continue
                }
            }
            UiEvent::Paste(text) => {
                pause_welcome_animation_for_input(app);
                let decision = self.paste_coordinator.decide_terminal_paste(&text);
                if !decision.should_forward() {
                    frame_requester.schedule_frame();
                    return Ok(RuntimeControl::Continue);
                }
                let _ = app.bottom_pane.handle_paste(&text);
                frame_requester.schedule_frame();
                RuntimeControl::Continue
            }
            UiEvent::Resize => {
                frame_requester.schedule_frame();
                RuntimeControl::Continue
            }
            UiEvent::Tick => {
                if handle_animation_tick(app, client).await {
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

impl RuntimeController {
    fn handle_text_paste_shortcut(&mut self, app: &mut TuiApp) -> bool {
        let text = match paste_clipboard_text() {
            Ok(text) => text,
            Err(_) => {
                self.paste_coordinator.clear();
                return false;
            }
        };
        self.paste_coordinator.record_shortcut_text(&text);
        let _ = app.bottom_pane.handle_paste(&text);
        true
    }
}

fn should_request_older_history_page(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::PageUp | KeyCode::Home)
}

pub(crate) enum RuntimeControl {
    Continue,
    Draw,
    Break,
}

#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
