use crate::app::TuiApp;
use crate::app::conversation::actions::execute_server_action;
use crate::state::NoticeLevel;
use crate::state::reducer::apply_server_message;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use agent_app_server_client::AppServerEvent;
use agent_protocol::{AppServerMessage, AppServerNotification, AppServerRequest};

pub(crate) fn handle_client_event(app: &mut TuiApp, event: AppServerEvent) {
    match event {
        AppServerEvent::Message(message) => handle_server_message(app, &message),
        AppServerEvent::Lagged { skipped } => {
            app.run_state.set_system_notice_level(
                format!("UI skipped {skipped} non-critical events while catching up"),
                NoticeLevel::Warn,
            );
        }
        AppServerEvent::Disconnected { message } => {
            app.push_cell(HistoryCell::from_message(
                "conversation",
                message,
                HistoryTone::Error,
            ));
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
