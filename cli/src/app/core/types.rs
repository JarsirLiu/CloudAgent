use crate::app::core::transcript_owner::TranscriptOwner;
use crate::app::runtime::terminal_projection::TerminalProjectionController;
use crate::state::RunState;
use crate::state::bottom_pane_controller::BottomPaneController;
use agent_core::AgentHost;
use agent_core::ConversationSummary;
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
    pub target_label: String,
    pub bootstrap: ConsoleBootstrap,
}

#[derive(Clone)]
pub enum AppServerTarget {
    LocalNode,
    HubNode { node_id: String },
}

impl AppServerTarget {
    pub fn label(&self) -> &'static str {
        match self {
            Self::LocalNode => "local-node",
            Self::HubNode { .. } => "hub-node",
        }
    }
}

#[derive(Clone)]
pub enum ConsoleBootstrap {
    LocalNode {
        address: String,
        program: OsString,
        args: Vec<OsString>,
        expected_data_root_dir: PathBuf,
    },
    Embedded {
        runtime: Arc<AgentHost>,
    },
}

pub(crate) struct TuiApp {
    pub(crate) conversation_id: String,
    pub(crate) conversation_summaries: Vec<ConversationSummary>,
    pub(crate) target_label: String,
    pub(crate) transcript_owner: TranscriptOwner,
    pub(crate) run_state: RunState,
    pub(crate) bottom_pane: BottomPaneController,
    pub(crate) terminal_projection: TerminalProjectionController,
    pub(crate) suppress_next_reset_notice: bool,
    pub(crate) welcome_animation_frame: u64,
    pub(crate) welcome_animation_pause_ticks: u8,
    pub(crate) workspace_root: PathBuf,
    pub(crate) conversation_store_dir: PathBuf,
}
