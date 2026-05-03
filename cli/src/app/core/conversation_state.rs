use crate::app::TuiApp;
use crate::state::reducer::TurnDispatch;
use crate::state::runtime_projection::RuntimeProjection;
use crate::state::{ConsoleState, RunState, ServerRequestState, TranscriptState};
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use agent_protocol::FrontendMode;

impl TuiApp {
    pub(crate) fn new(conversation_id: String, connection_label: &str, workspace_root: std::path::PathBuf) -> Self {
        Self {
            conversation_id,
            conversation_summaries: Vec::new(),
            connection_label: connection_label.to_string(),
            console_state: ConsoleState::new(),
            server_request_state: ServerRequestState::default(),
            transcript_state: TranscriptState::default(),
            run_state: RunState::new(connection_label),
            runtime_projection: RuntimeProjection::new(),
            input_pane: crate::ui::widgets::input_pane::InputPane::new(),
            welcome_animation_frame: 0,
            welcome_animation_pause_ticks: 0,
            pending_history_cells: std::collections::VecDeque::new(),
            pending_history_rebuild: false,
            session_picker_requested: false,
            delete_picker_requested: false,
            workspace_root,
        }
    }

    pub(crate) fn reset_local_view(&mut self) {
        self.console_state = ConsoleState::new();
        self.server_request_state = ServerRequestState::default();
        self.transcript_state = TranscriptState::default();
        self.run_state = RunState::new(&self.connection_label);
        self.runtime_projection = RuntimeProjection::new();
        self.run_state.history_loaded = true;
        self.input_pane.clear_views();
        self.welcome_animation_frame = 0;
        self.welcome_animation_pause_ticks = 0;
        self.pending_history_cells.clear();
        self.pending_history_rebuild = false;
    }

    pub(crate) fn switch_conversation(&mut self, conversation_id: String) {
        self.conversation_id = conversation_id;
        self.reset_local_view();
    }

    pub(crate) fn set_conversation_summaries(
        &mut self,
        conversation_summaries: Vec<agent_protocol::ConversationSummary>,
    ) {
        self.conversation_summaries = conversation_summaries;
    }

    pub(crate) fn set_mode(&mut self, mode: FrontendMode) {
        self.console_state.mode = mode;
    }

    pub(crate) fn apply_turn_dispatch(&mut self, dispatch: TurnDispatch) {
        match dispatch {
            TurnDispatch::Completed => self.flush_active_cell_to_transcript(),
            TurnDispatch::Failed { error } => {
                self.flush_active_cell_to_transcript();
                self.push_cell(HistoryCell::from_message(
                    "turn",
                    format!("failed: {error}"),
                    HistoryTone::Error,
                ));
            }
            TurnDispatch::Cancelled { reason } => {
                self.flush_active_cell_to_transcript();
                self.push_cell(HistoryCell::from_message("turn", reason, HistoryTone::Warning));
            }
        }
    }
}
