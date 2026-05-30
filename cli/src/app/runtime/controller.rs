use crate::app::TuiApp;
use crate::app::clipboard_paste::paste_image_to_temp_png;
use crate::app::conversation::actions::handle_tui_input;
use crate::app::conversation::event_router;
use crate::app::runtime::lifecycle::{handle_animation_tick, pause_welcome_animation_for_input};
use crate::state::NoticeLevel;
use crate::terminal::{FrameRequester, UiEvent};
use agent_app_server_client::{AppServerClient, AppServerEvent};
use agent_protocol::{AppServerMessage, AppServerNotification};
use anyhow::Result;
use std::path::PathBuf;

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
                handle_paste_event(app, &text, paste_image_to_temp_png);
                frame_requester.schedule_frame();
                RuntimeControl::Continue
            }
            UiEvent::Resize => {
                app.terminal_projection.request_history_replay();
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

fn handle_paste_event<F>(app: &mut TuiApp, text: &str, paste_image: F)
where
    F: FnOnce() -> Result<PathBuf, crate::app::clipboard_paste::PasteImageError>,
{
    match paste_image() {
        Ok(path) => {
            let _ = app.bottom_pane.attach_image(path);
        }
        Err(err) => {
            if !text.is_empty() {
                let _ = app.bottom_pane.handle_paste(text);
                return;
            }

            app.bottom_pane
                .show_transient_notice(NoticeLevel::Warn, format!("Failed to paste image: {err}"));
        }
    }
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
    use super::{coalesce_client_events, handle_paste_event};
    use crate::app::TuiApp;
    use agent_app_server_client::AppServerEvent;
    use agent_protocol::{AppServerMessage, AppServerNotification};
    use std::path::PathBuf;

    fn test_app() -> TuiApp {
        TuiApp::new(
            "default".to_string(),
            "test",
            PathBuf::from("D:\\learn\\gifti\\cloudagent"),
            PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
            false,
            "WorkspaceWrite".to_string(),
        )
    }

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

    #[test]
    fn paste_event_with_text_falls_back_to_text_when_no_image_is_available() {
        let mut app = test_app();

        handle_paste_event(&mut app, "hello", || {
            Err(crate::app::clipboard_paste::PasteImageError::NoImage(
                "missing".to_string(),
            ))
        });

        let lines =
            app.bottom_pane
                .render_lines_for_test(agent_protocol::FrontendMode::Idle, "", "", 80);
        let rendered = lines
            .0
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("hello"));
    }

    #[test]
    fn paste_event_with_text_prefers_clipboard_image_when_available() {
        let mut app = test_app();
        let image_path = std::env::temp_dir().join("runtime-paste-priority-test.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 255, 255]))
            .save(&image_path)
            .expect("save temp image");

        handle_paste_event(&mut app, "ignored text representation", || Ok(image_path));

        let lines =
            app.bottom_pane
                .render_lines_for_test(agent_protocol::FrontendMode::Idle, "", "", 80);
        let rendered = lines
            .0
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("[Image #1]"));
        assert!(!rendered.contains("ignored text representation"));
    }

    #[test]
    fn empty_paste_event_can_attach_clipboard_image() {
        let mut app = test_app();
        let image_path = std::env::temp_dir().join("runtime-paste-test.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 0, 255]))
            .save(&image_path)
            .expect("save temp image");

        handle_paste_event(&mut app, "", || Ok(image_path.clone()));

        let lines =
            app.bottom_pane
                .render_lines_for_test(agent_protocol::FrontendMode::Idle, "", "", 80);
        let rendered = lines
            .0
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("[Image #1]"));
    }

    #[test]
    fn empty_paste_event_surfaces_warning_when_clipboard_image_unavailable() {
        let mut app = test_app();

        handle_paste_event(&mut app, "", || {
            Err(crate::app::clipboard_paste::PasteImageError::NoImage(
                "missing".to_string(),
            ))
        });

        let status = app.bottom_pane.build_status_view_model(&app);
        let banner = status
            .live_banner
            .expect("warning banner should be created");
        assert!(banner.contains("Failed to paste image"));
        assert!(banner.contains("missing"));
    }
}
