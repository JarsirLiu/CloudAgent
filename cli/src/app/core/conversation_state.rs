use crate::app::TuiApp;
use crate::app::core::transcript_owner::TranscriptOwner;
use crate::app::runtime::terminal_projection::TerminalProjectionController;
use crate::state::RunState;
use crate::state::bottom_pane_controller::BottomPaneController;
use crate::state::reducer::TurnDispatch;
use crate::ui::transcript_render_cache::TranscriptRenderCache;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
use agent_core::conversation::{ConversationSummary, InputItem};
use agent_core::turn::{ModelRetryStage, TurnItemKind};
use agent_protocol::{
    ConversationActiveFlag, ConversationViewSnapshot, ConversationViewStatus, FrontendMode,
    RequestId,
};

impl TuiApp {
    pub(crate) fn new(
        conversation_id: String,
        target_label: &str,
        workspace_root: std::path::PathBuf,
        conversation_store_dir: std::path::PathBuf,
        initial_filter_enabled: bool,
        initial_permission_mode: String,
    ) -> Self {
        let mut run_state = RunState::new(target_label);
        run_state.pre_llm_filter_enabled = initial_filter_enabled;
        run_state.permission_mode = initial_permission_mode;
        Self {
            conversation_id,
            conversation_summaries: Vec::new(),
            target_label: target_label.to_string(),
            transcript_owner: TranscriptOwner::default(),
            transcript_scroll: Default::default(),
            transcript_render_cache: TranscriptRenderCache::default(),
            run_state,
            bottom_pane: BottomPaneController::new(),
            terminal_projection: TerminalProjectionController,
            suppress_next_reset_notice: false,
            welcome_animation_frame: 0,
            welcome_animation_pause_ticks: 0,
            workspace_root,
            conversation_store_dir,
            conversation_history_turn_limit: Some(30),
        }
    }

    pub(crate) fn reset_local_view(&mut self) {
        let filter_enabled = self.run_state.pre_llm_filter_enabled;
        let permission_mode = self.run_state.permission_mode.clone();
        self.transcript_owner.clear();
        self.transcript_scroll.reset();
        self.transcript_render_cache.clear();
        self.run_state = RunState::new(&self.target_label);
        self.run_state.pre_llm_filter_enabled = filter_enabled;
        self.run_state.permission_mode = permission_mode;
        self.bottom_pane = BottomPaneController::new();
        self.terminal_projection.reset();
        self.bottom_pane.clear_views();
        self.welcome_animation_frame = 0;
        self.welcome_animation_pause_ticks = 0;
    }

    pub(crate) fn arm_reset_notice_suppression(&mut self) {
        self.suppress_next_reset_notice = true;
    }

    pub(crate) fn should_suppress_notice(&mut self, label: &str, message: &str) -> bool {
        if self.suppress_next_reset_notice
            && label == "conversation"
            && message.trim().eq_ignore_ascii_case("conversation reset")
        {
            self.suppress_next_reset_notice = false;
            return true;
        }
        false
    }

    pub(crate) fn switch_conversation(&mut self, conversation_id: String) {
        self.conversation_id = conversation_id;
        self.reset_local_view();
    }

    pub(crate) fn handle_conversation_list(
        &mut self,
        conversation_summaries: Vec<ConversationSummary>,
    ) {
        self.conversation_summaries = conversation_summaries.clone();
        let _ = self
            .bottom_pane
            .present_requested_session_picker(conversation_summaries, &self.conversation_id);
    }

    pub(crate) fn on_server_turn_started(&mut self) {
        self.run_state.last_turn_usage = None;
        self.run_state.total_turn_usage = None;
        self.run_state.model_context_window = None;
        self.bottom_pane.on_turn_started();
    }

    pub(crate) fn on_server_tool_finished(&mut self) {
        self.bottom_pane.on_tool_finished();
    }

    pub(crate) fn on_server_retrying(
        &mut self,
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    ) {
        self.bottom_pane
            .on_model_retrying(stage, attempt, next_delay_ms);
    }

    pub(crate) fn on_server_active_item_started(
        &mut self,
        item_id: &str,
        kind: &TurnItemKind,
        title: Option<&str>,
    ) {
        self.bottom_pane
            .on_active_item_started(item_id, kind, title);
    }

    pub(crate) fn show_server_request_prompt(
        &mut self,
        request: crate::ui::widgets::input_pane::ServerRequestInlineState,
    ) {
        self.bottom_pane.set_server_request(request);
    }

    pub(crate) fn clear_server_request_view(&mut self) {
        self.bottom_pane.clear_server_request();
    }

    pub(crate) fn dismiss_server_request_view(&mut self, request_id: &RequestId) {
        self.bottom_pane.dismiss_server_request(request_id);
    }

    pub(crate) fn prepare_submitted_turn(&mut self, content: &[InputItem]) {
        self.run_state.last_turn_usage = None;
        self.run_state.total_turn_usage = None;
        self.run_state.model_context_window = None;
        self.run_state.turn_lifecycle.begin_submit(content);
        self.bottom_pane.prepare_for_submit();
        self.transcript_owner
            .start_local_user(content.to_vec(), self.run_state.expand_tool_details);
        self.transcript_scroll.reset();
    }

    pub(crate) fn apply_turn_dispatch(&mut self, dispatch: TurnDispatch) {
        self.run_state.turn_lifecycle.finish_turn();
        self.bottom_pane.on_turn_finished();
        match dispatch {
            TurnDispatch::Completed => {
                self.run_state.turn_lifecycle.clear_pending_submission();
                if let Some(turn_id) = self.transcript_owner.active_turn_id().map(str::to_owned) {
                    self.transcript_owner
                        .complete_turn(turn_id, self.run_state.expand_tool_details);
                } else {
                    self.transcript_owner
                        .clear_active_turn(self.run_state.expand_tool_details);
                }
                self.transcript_scroll.reset();
            }
            TurnDispatch::Failed { error } => {
                let restored_draft = if let Some(content) =
                    self.run_state.turn_lifecycle.take_pending_submission()
                {
                    self.bottom_pane.restore_submission(&content);
                    true
                } else {
                    false
                };
                self.transcript_owner
                    .clear_active_turn(self.run_state.expand_tool_details);
                let message = if restored_draft {
                    format!("failed: {error}\ndraft restored for retry")
                } else {
                    format!("failed: {error}")
                };
                self.push_live_cell(HistoryCell::info("turn", message, HistoryTone::Error));
                self.transcript_scroll.reset();
            }
            TurnDispatch::Cancelled { reason } => {
                self.run_state.turn_lifecycle.clear_pending_submission();
                self.transcript_owner
                    .clear_active_turn(self.run_state.expand_tool_details);
                self.push_live_cell(HistoryCell::info("turn", reason, HistoryTone::Warning));
                self.transcript_scroll.reset();
            }
        }
    }

    pub(crate) fn push_live_cell(&mut self, cell: HistoryCell) {
        self.transcript_owner.push_live_cell(cell);
    }

    pub(crate) fn current_mode(&self) -> FrontendMode {
        self.run_state
            .conversation_view_snapshot
            .as_ref()
            .map(frontend_mode_from_snapshot)
            .unwrap_or(FrontendMode::Idle)
    }

    pub(crate) fn apply_conversation_view_snapshot(&mut self, snapshot: ConversationViewSnapshot) {
        let mode = frontend_mode_from_snapshot(&snapshot);
        self.run_state.conversation_view_snapshot = Some(snapshot);
        self.bottom_pane.sync_frontend_mode(mode);
    }

    pub(crate) fn can_submit_turn(&self) -> bool {
        self.current_mode() == FrontendMode::Idle
    }

    #[cfg(test)]
    pub(crate) fn live_cells(&self) -> &[HistoryCell] {
        self.transcript_owner.live_cells()
    }
}

fn frontend_mode_from_snapshot(
    snapshot: &agent_protocol::ConversationViewSnapshot,
) -> FrontendMode {
    match &snapshot.status {
        ConversationViewStatus::Active { flags, .. }
            if flags.contains(&ConversationActiveFlag::WaitingOnApproval) =>
        {
            FrontendMode::WaitingForServerRequest
        }
        ConversationViewStatus::Active { .. } => FrontendMode::Running,
        ConversationViewStatus::NotLoaded
        | ConversationViewStatus::Idle
        | ConversationViewStatus::SystemError { .. } => FrontendMode::Idle,
    }
}

#[cfg(test)]
pub(crate) fn conversation_view_snapshot_for_test(
    conversation_id: &str,
    mode: FrontendMode,
) -> agent_protocol::ConversationViewSnapshot {
    let status = match mode {
        FrontendMode::Idle => ConversationViewStatus::Idle,
        FrontendMode::Running => ConversationViewStatus::Active {
            active_turn_id: None,
            flags: vec![ConversationActiveFlag::RunningTurn],
        },
        FrontendMode::WaitingForServerRequest => ConversationViewStatus::Active {
            active_turn_id: None,
            flags: vec![
                ConversationActiveFlag::RunningTurn,
                ConversationActiveFlag::WaitingOnApproval,
            ],
        },
    };
    agent_protocol::ConversationViewSnapshot {
        conversation_id: conversation_id.to_string(),
        status,
        active_turn: None,
        pending_requests: Vec::new(),
        message_count: 0,
        updated_at_ms: 0,
    }
}
