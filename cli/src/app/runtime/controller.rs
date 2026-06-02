use crate::app::TuiApp;
use crate::app::conversation::actions::handle_tui_input;
use crate::app::conversation::event_router;
use crate::app::runtime::lifecycle::{handle_animation_tick, pause_welcome_animation_for_input};
use crate::terminal::{FrameRequester, UiEvent};
use agent_app_server_client::{AppServerClient, AppServerEvent};
use agent_protocol::{AppServerMessage, AppServerNotification};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

pub(crate) struct RuntimeController;

impl RuntimeController {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn handle_client_event(
        &mut self,
        app: &mut TuiApp,
        client: &mut AppServerClient,
        event: AppServerEvent,
        frame_requester: &FrameRequester,
    ) {
        let events = collect_client_events(client, event);
        for event in events {
            event_router::handle_client_event(app, event);
        }
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
            UiEvent::Paste(text) => {
                pause_welcome_animation_for_input(app);
                let _ = app.bottom_pane.handle_paste(&text);
                frame_requester.schedule_frame();
                RuntimeControl::Continue
            }
            UiEvent::Mouse(mouse) => {
                if app.transcript_scroll.handle_mouse(mouse) {
                    frame_requester.schedule_frame();
                }
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

fn should_request_older_history_page(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::PageUp | KeyCode::Home)
}

pub(crate) enum RuntimeControl {
    Continue,
    Draw,
    Break,
}

fn collect_client_events(
    client: &mut AppServerClient,
    first: AppServerEvent,
) -> Vec<AppServerEvent> {
    let mut events = vec![first];
    while let Some(event) = client.try_next_event() {
        events.push(event);
    }
    coalesce_client_events(events)
}

fn coalesce_client_events(events: Vec<AppServerEvent>) -> Vec<AppServerEvent> {
    let mut coalesced = Vec::with_capacity(events.len());
    let mut skipped = 0usize;
    for event in events {
        match event {
            AppServerEvent::Lagged { skipped: more } => {
                skipped = skipped.saturating_add(more);
            }
            AppServerEvent::Disconnected { .. } => {
                if skipped > 0 {
                    tracing::warn!(
                        skipped,
                        "app-server event consumer lagged; dropping ignored events"
                    );
                    skipped = 0;
                }
                coalesced.push(event);
            }
            AppServerEvent::Message(message) => {
                if skipped > 0 {
                    tracing::warn!(
                        skipped,
                        "app-server event consumer lagged; dropping ignored events"
                    );
                    skipped = 0;
                }
                if let Some(last) = coalesced.last_mut()
                    && try_merge_messages(last, &message)
                {
                    continue;
                }
                coalesced.push(AppServerEvent::Message(message));
            }
        }
    }
    if skipped > 0 {
        tracing::warn!(
            skipped,
            "app-server event consumer lagged; dropping ignored events"
        );
    }
    coalesced
}

fn try_merge_messages(existing: &mut AppServerEvent, next: &AppServerMessage) -> bool {
    let AppServerEvent::Message(existing_message) = existing else {
        return false;
    };
    match (existing_message, next) {
        (
            AppServerMessage::Notification(AppServerNotification::CommandExecutionOutputDelta {
                conversation_id: left_conversation_id,
                turn_id: left_turn_id,
                item_id: left_item_id,
                call_id: left_call_id,
                delta: left_delta,
            }),
            AppServerMessage::Notification(AppServerNotification::CommandExecutionOutputDelta {
                conversation_id: right_conversation_id,
                turn_id: right_turn_id,
                item_id: right_item_id,
                call_id: right_call_id,
                delta: right_delta,
            }),
        ) if left_conversation_id == right_conversation_id
            && left_turn_id == right_turn_id
            && left_item_id == right_item_id
            && left_call_id == right_call_id =>
        {
            left_delta.push_str(right_delta);
            true
        }
        (
            AppServerMessage::Notification(AppServerNotification::ToolOutputDelta {
                conversation_id: left_conversation_id,
                turn_id: left_turn_id,
                item_id: left_item_id,
                call_id: left_call_id,
                delta: left_delta,
            }),
            AppServerMessage::Notification(AppServerNotification::ToolOutputDelta {
                conversation_id: right_conversation_id,
                turn_id: right_turn_id,
                item_id: right_item_id,
                call_id: right_call_id,
                delta: right_delta,
            }),
        ) if left_conversation_id == right_conversation_id
            && left_turn_id == right_turn_id
            && left_item_id == right_item_id
            && left_call_id == right_call_id =>
        {
            left_delta.push_str(right_delta);
            true
        }
        (
            AppServerMessage::Notification(AppServerNotification::FileChangeOutputDelta {
                conversation_id: left_conversation_id,
                turn_id: left_turn_id,
                item_id: left_item_id,
                call_id: left_call_id,
                delta: left_delta,
            }),
            AppServerMessage::Notification(AppServerNotification::FileChangeOutputDelta {
                conversation_id: right_conversation_id,
                turn_id: right_turn_id,
                item_id: right_item_id,
                call_id: right_call_id,
                delta: right_delta,
            }),
        ) if left_conversation_id == right_conversation_id
            && left_turn_id == right_turn_id
            && left_item_id == right_item_id
            && left_call_id == right_call_id =>
        {
            left_delta.push_str(right_delta);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::coalesce_client_events;
    use agent_app_server_client::AppServerEvent;
    use agent_protocol::{AppServerMessage, AppServerNotification};

    fn command_delta(item_id: &str, delta: &str) -> AppServerEvent {
        AppServerEvent::Message(AppServerMessage::Notification(
            AppServerNotification::CommandExecutionOutputDelta {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: item_id.to_string(),
                call_id: Some("call-1".to_string()),
                delta: delta.to_string(),
            },
        ))
    }

    #[test]
    fn coalesces_adjacent_command_output_deltas_for_same_item() {
        let events = vec![
            command_delta("tool:1", "hello "),
            command_delta("tool:1", "world"),
        ];

        let coalesced = coalesce_client_events(events);

        assert_eq!(coalesced.len(), 1);
        let AppServerEvent::Message(AppServerMessage::Notification(
            AppServerNotification::CommandExecutionOutputDelta { delta, .. },
        )) = &coalesced[0]
        else {
            panic!("expected merged command delta");
        };
        assert_eq!(delta, "hello world");
    }

    #[test]
    fn does_not_coalesce_command_output_across_items() {
        let events = vec![
            command_delta("tool:1", "hello "),
            command_delta("tool:2", "world"),
        ];

        let coalesced = coalesce_client_events(events);

        assert_eq!(coalesced.len(), 2);
    }

    #[test]
    fn drops_lagged_markers_from_user_visible_event_stream() {
        let events = vec![
            AppServerEvent::Lagged { skipped: 3 },
            command_delta("tool:1", "done"),
        ];

        let coalesced = coalesce_client_events(events);

        assert_eq!(coalesced.len(), 1);
        assert!(matches!(coalesced[0], AppServerEvent::Message(_)));
    }
}
