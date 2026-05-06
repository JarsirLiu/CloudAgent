use crate::app::TuiApp;
use crate::app::core::transcript_owner::TranscriptOwner;
use crate::state::reducer::TurnDispatch;
use crate::state::runtime_projection::RuntimeProjection;
use crate::state::{ConsoleState, RunState, ServerRequestState, TranscriptState};
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use agent_protocol::FrontendMode;

impl TuiApp {
    pub(crate) fn new(
        conversation_id: String,
        connection_label: &str,
        workspace_root: std::path::PathBuf,
        conversation_store_dir: std::path::PathBuf,
        initial_filter_enabled: bool,
        initial_permission_mode: String,
    ) -> Self {
        let mut run_state = RunState::new(connection_label);
        run_state.pre_llm_filter_enabled = initial_filter_enabled;
        run_state.permission_mode = initial_permission_mode;
        Self {
            conversation_id,
            conversation_summaries: Vec::new(),
            connection_label: connection_label.to_string(),
            console_state: ConsoleState::new(),
            server_request_state: ServerRequestState::default(),
            transcript_state: TranscriptState::default(),
            transcript_owner: TranscriptOwner::default(),
            run_state,
            runtime_projection: RuntimeProjection::new(),
            input_pane: crate::ui::widgets::input_pane::InputPane::new(),
            welcome_animation_frame: 0,
            welcome_animation_pause_ticks: 0,
            session_picker_requested: false,
            delete_picker_requested: false,
            workspace_root,
            conversation_store_dir,
        }
    }

    pub(crate) fn reset_local_view(&mut self) {
        let filter_enabled = self.run_state.pre_llm_filter_enabled;
        let permission_mode = self.run_state.permission_mode.clone();
        self.console_state = ConsoleState::new();
        self.server_request_state = ServerRequestState::default();
        self.transcript_state = TranscriptState::default();
        self.transcript_state.reset_scroll();
        self.transcript_owner.clear();
        self.run_state = RunState::new(&self.connection_label);
        self.run_state.pre_llm_filter_enabled = filter_enabled;
        self.run_state.permission_mode = permission_mode;
        self.runtime_projection = RuntimeProjection::new();
        self.run_state.history_loaded = true;
        self.input_pane.clear_views();
        self.welcome_animation_frame = 0;
        self.welcome_animation_pause_ticks = 0;
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
        if mode == FrontendMode::Idle {
            self.transcript_state.clear_inline_viewport_height_lock();
        }
    }

    pub(crate) fn apply_turn_dispatch(&mut self, dispatch: TurnDispatch) {
        match dispatch {
            TurnDispatch::Completed => {
                if let Some(turn_id) = self.transcript_owner.active_turn_id().map(str::to_owned) {
                    self.transcript_owner
                        .complete_turn(turn_id, self.run_state.expand_tool_details);
                } else {
                    self.transcript_owner
                        .clear_active_turn(self.run_state.expand_tool_details);
                }
            }
            TurnDispatch::Failed { error } => {
                self.push_live_cell(HistoryCell::info(
                    "turn",
                    format!("failed: {error}"),
                    HistoryTone::Error,
                ));
            }
            TurnDispatch::Cancelled { reason } => {
                self.push_live_cell(HistoryCell::info("turn", reason, HistoryTone::Warning));
            }
        }
    }
}
