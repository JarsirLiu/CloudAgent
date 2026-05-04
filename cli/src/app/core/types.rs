use crate::state::runtime_projection::RuntimeProjection;
use crate::state::{ConsoleState, RunState, ServerRequestState, TranscriptState};
use crate::ui::widgets::history_cell::HistoryCell;
use crate::ui::widgets::input_pane::InputPane;
use agent_protocol::ConversationSummary;
use agent_core::AgentHost;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct ConsoleConfig {
    pub conversation_id: String,
    pub workspace_root: PathBuf,
    pub conversation_store_dir: PathBuf,
    pub initial_filter_enabled: bool,
    pub initial_permission_mode: String,
    pub auto_approve: bool,
    pub auto_approve_reason: Option<String>,
    pub connection: ConsoleConnection,
}

#[derive(Clone)]
pub enum ConsoleConnection {
    InProcess { runtime: Arc<AgentHost> },
    Stdio { program: OsString, args: Vec<OsString> },
}

impl ConsoleConnection {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::InProcess { .. } => "in-process",
            Self::Stdio { .. } => "stdio-bridge",
        }
    }
}

pub(crate) struct TuiApp {
    pub(crate) conversation_id: String,
    pub(crate) conversation_summaries: Vec<ConversationSummary>,
    pub(crate) connection_label: String,
    pub(crate) console_state: ConsoleState,
    pub(crate) server_request_state: ServerRequestState,
    pub(crate) transcript_state: TranscriptState,
    pub(crate) run_state: RunState,
    pub(crate) runtime_projection: RuntimeProjection,
    pub(crate) input_pane: InputPane,
    pub(crate) welcome_animation_frame: u64,
    pub(crate) welcome_animation_pause_ticks: u8,
    pub(crate) pending_history_cells: VecDeque<HistoryCell>,
    pub(crate) pending_history_rebuild: bool,
    pub(crate) session_picker_requested: bool,
    pub(crate) delete_picker_requested: bool,
    pub(crate) workspace_root: PathBuf,
    pub(crate) conversation_store_dir: PathBuf,
}


