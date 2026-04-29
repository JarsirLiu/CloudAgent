pub mod reducer;
pub mod selectors;

use crate::ui::widgets::history_cell::{HistoryCell, Transcript};
use agent_protocol::{
    AppServerMessage, AppServerNotification, AppServerRequest, FrontendMode, HistoryEntry,
    RequestId, TurnEvent,
};

#[derive(Clone, Debug)]
pub struct ConsoleState {
    pub mode: FrontendMode,
}

impl ConsoleState {
    pub fn new() -> Self {
        Self {
            mode: FrontendMode::Idle,
        }
    }

    pub fn can_submit_turn(&self) -> bool {
        self.mode == FrontendMode::Idle
    }
}

#[derive(Clone, Debug, Default)]
pub struct ServerRequestState {
    pub pending_server_request_id: Option<RequestId>,
}

#[derive(Default)]
pub struct TranscriptState {
    pub transcript: Transcript,
    pub scroll: usize,
    pub viewport_height: usize,
    pub viewport_width: usize,
    pub active_item_id: Option<String>,
    pub active_item_kind: Option<agent_protocol::TurnItemKind>,
    pub active_cell: Option<HistoryCell>,
    pub last_copyable_output: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RunState {
    pub history_loaded: bool,
    pub event_log_loaded: bool,
    pub history_snapshot: Option<Vec<HistoryEntry>>,
    pub event_log_snapshot: Option<Vec<TurnEvent>>,
    pub status_notice: Option<String>,
    pub last_message_count: usize,
    pub last_tool_name: Option<String>,
    pub should_exit: bool,
}

impl RunState {
    pub fn new(connection_label: &str) -> Self {
        Self {
            history_loaded: false,
            event_log_loaded: false,
            history_snapshot: None,
            event_log_snapshot: None,
            status_notice: Some(format!("Connected via {connection_label}")),
            last_message_count: 0,
            last_tool_name: None,
            should_exit: false,
        }
    }
}

pub fn update_core_state_from_message(
    console: &mut ConsoleState,
    server_request: &mut ServerRequestState,
    message: &AppServerMessage,
) {
    match message {
        AppServerMessage::Notification(notification) => match notification {
            AppServerNotification::FrontendStateChanged { mode, .. } => console.mode = *mode,
            AppServerNotification::TurnCompleted { .. }
            | AppServerNotification::TurnFailed { .. }
            | AppServerNotification::TurnCancelled { .. } => {
                console.mode = FrontendMode::Idle;
                server_request.pending_server_request_id = None;
            }
            _ => {}
        },
        AppServerMessage::Request(AppServerRequest::ServerRequest { request_id, .. }) => {
            console.mode = FrontendMode::WaitingForServerRequest;
            server_request.pending_server_request_id = Some(request_id.clone());
        }
    }
}
