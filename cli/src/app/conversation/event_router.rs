use crate::app::TuiApp;
use crate::app::conversation::actions::execute_server_action;
use crate::state::NoticeLevel;
use crate::state::reducer::TurnDispatch;
use crate::state::reducer::apply_server_message;
use agent_app_server_client::AppServerEvent;
use agent_protocol::{AppServerMessage, AppServerNotification, AppServerRequest};

pub(crate) fn handle_client_event(app: &mut TuiApp, event: AppServerEvent) {
    match event {
        AppServerEvent::Message(message) => handle_server_message(app, &message),
        AppServerEvent::Lagged { .. } => {}
        AppServerEvent::Disconnected { message } => {
            if !app.can_submit_turn() {
                app.apply_turn_dispatch(TurnDispatch::Failed {
                    error: message.clone(),
                });
            }
            app.bottom_pane
                .show_transient_notice(NoticeLevel::Error, message);
            app.run_state.should_exit = true;
        }
    }
}

fn handle_server_message(app: &mut TuiApp, message: &AppServerMessage) {
    if !should_apply_server_message(app, message) {
        return;
    }
    let reduced = apply_server_message(message);
    for action in reduced.actions {
        execute_server_action(app, action);
    }
}

fn should_apply_server_message(app: &TuiApp, message: &AppServerMessage) -> bool {
    match message {
        AppServerMessage::Notification(AppServerNotification::ConversationList { .. })
        | AppServerMessage::Notification(AppServerNotification::ConversationSwitched { .. }) => {
            true
        }
        AppServerMessage::Notification(notification) => {
            notification.conversation_id() == app.conversation_id
        }
        AppServerMessage::Request(AppServerRequest::ServerRequest {
            conversation_id, ..
        }) => conversation_id == &app.conversation_id,
    }
}

#[cfg(test)]
mod tests {
    use super::handle_client_event;
    use crate::app::TuiApp;
    use agent_app_server_client::AppServerEvent;
    use std::path::PathBuf;

    fn test_app() -> TuiApp {
        TuiApp::new(
            "default".to_string(),
            "test",
            PathBuf::from("D:\\learn\\gifti\\cloudagent"),
            PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
            false,
            "ReadOnly".to_string(),
        )
    }

    #[test]
    fn disconnected_event_uses_error_status_banner_instead_of_transcript_cell() {
        let mut app = test_app();

        handle_client_event(
            &mut app,
            AppServerEvent::Disconnected {
                message: "worker app server closed unexpectedly".to_string(),
            },
        );

        let status = app.bottom_pane.build_status_view_model(&app);
        assert_eq!(
            status.live_banner.as_deref(),
            Some("worker app server closed unexpectedly")
        );
        assert_eq!(
            status.live_banner_level,
            Some(crate::state::NoticeLevel::Error)
        );
        assert!(app.transcript_owner.active_cell().is_none());
        assert!(app.run_state.should_exit);
    }
}
