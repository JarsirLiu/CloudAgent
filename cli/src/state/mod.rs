pub mod reducer;
pub mod selectors;

use crate::ui::widgets::history_cell::{HistoryCell, Transcript};
use agent_protocol::{ConversationTurn, FrontendMode, ModelUsage, RequestId};

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
    pub active_request_id: Option<RequestId>,
    pub action_required: bool,
}

#[derive(Default)]
pub struct TranscriptState {
    pub transcript: Transcript,
    pub active_item_id: Option<String>,
    pub active_item_kind: Option<agent_protocol::TurnItemKind>,
    pub active_cell: Option<HistoryCell>,
    pub last_copyable_output: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RunState {
    pub history_loaded: bool,
    pub history_snapshot: Option<Vec<ConversationTurn>>,
    pub status_notice: Option<String>,
    pub last_tool_name: Option<String>,
    pub last_turn_usage: Option<ModelUsage>,
    pub total_turn_usage: Option<ModelUsage>,
    pub model_context_window: Option<u64>,
    pub should_exit: bool,
}

impl RunState {
    pub fn new(connection_label: &str) -> Self {
        Self {
            history_loaded: false,
            history_snapshot: None,
            status_notice: Some(format!("Connected via {connection_label}")),
            last_tool_name: None,
            last_turn_usage: None,
            total_turn_usage: None,
            model_context_window: None,
            should_exit: false,
        }
    }
}
