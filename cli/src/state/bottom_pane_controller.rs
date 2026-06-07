use crate::app::TuiApp;
use crate::state::NoticeLevel;
use crate::state::bottom_pane_runtime::BottomPaneRuntimeState;
use crate::state::selectors::status_text_from_mode;
use crate::terminal::Frame;
use crate::ui::widgets::gateway_panel::WeixinLoginSessionView;
use crate::ui::widgets::input_pane::{
    InputPane, InputPaneAction, InputPaneRenderResult, ServerRequestInlineState,
};
use crate::ui::widgets::session_picker::SessionPickerMode;
use crate::ui::widgets::weixin_binding_view::WeixinBindingViewModel;
use agent_core::InputItem;
use agent_core::SkillMetadata;
use agent_core::{ConversationSummary, ModelRetryStage, TurnItemKind};
use agent_protocol::{FrontendMode, PlatformConfigResponse, PlatformControlEntry, RequestId};
use config::ReasoningEffort;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) struct StatusViewModel {
    pub(crate) indicator: Option<String>,
    pub(crate) text: String,
    pub(crate) runtime_hint: Option<String>,
    pub(crate) meta: String,
    pub(crate) hint_meta: String,
    pub(crate) live_banner: Option<String>,
    pub(crate) live_banner_level: Option<NoticeLevel>,
}

pub(crate) struct BottomPaneController {
    runtime: BottomPaneRuntimeState,
    input_pane: InputPane,
    pending_session_picker: Option<SessionPickerMode>,
}

impl BottomPaneController {
    pub(crate) fn new() -> Self {
        Self {
            runtime: BottomPaneRuntimeState::default(),
            input_pane: InputPane::new(),
            pending_session_picker: None,
        }
    }

    pub(crate) fn on_turn_started(&mut self) {
        self.runtime.on_turn_started();
    }

    pub(crate) fn on_tool_finished(&mut self) {
        self.runtime.on_tool_finished();
    }

    pub(crate) fn on_command_output_delta(&mut self, item_id: Option<&str>, delta: &str) {
        self.runtime.on_command_output_delta(item_id, delta);
    }

    pub(crate) fn on_command_finished(&mut self, item_id: &str) {
        self.runtime.on_command_finished(item_id);
    }

    pub(crate) fn on_context_compaction_started(&mut self, estimated_tokens: u64) {
        self.runtime.on_context_compaction_started(estimated_tokens);
    }

    pub(crate) fn on_context_compaction_finished(&mut self) {
        self.runtime.on_context_compaction_finished();
    }

    pub(crate) fn on_turn_finished(&mut self) {
        self.runtime.on_turn_finished();
    }

    pub(crate) fn on_model_retrying(
        &mut self,
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    ) {
        self.runtime
            .on_model_retrying(stage, attempt, next_delay_ms);
    }

    pub(crate) fn on_active_item_started(
        &mut self,
        item_id: &str,
        kind: &TurnItemKind,
        title: Option<&str>,
    ) {
        self.runtime.on_active_item_started(item_id, kind, title);
    }

    pub(crate) fn prepare_for_submit(&mut self) {
        self.clear_views();
        self.clear_composer();
        self.on_turn_started();
    }

    pub(crate) fn sync_frontend_mode(&mut self, mode: FrontendMode) {
        self.runtime.sync_frontend_mode(mode);
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<InputPaneAction> {
        self.input_pane.handle_key(key)
    }

    pub(crate) fn handle_paste(&mut self, text: &str) -> Option<InputPaneAction> {
        self.input_pane.handle_paste(text)
    }

    pub(crate) fn handle_tick(&mut self) -> bool {
        let mut needs_redraw = self.input_pane.handle_tick();
        if self.runtime.handle_tick() {
            needs_redraw = true;
        }
        needs_redraw
    }

    pub(crate) fn next_paste_flush_delay(&self) -> Option<Duration> {
        self.input_pane.next_paste_flush_delay()
    }

    pub(crate) fn composer_has_selection(&self) -> bool {
        self.input_pane.composer_has_selection()
    }

    pub(crate) fn should_capture_global_paste_shortcut(&self) -> bool {
        self.input_pane.should_capture_global_paste_shortcut()
    }

    pub(crate) fn composer_is_empty(&self) -> bool {
        self.input_pane.composer_is_empty()
    }

    pub(crate) fn attach_image(&mut self, path: PathBuf) -> bool {
        self.input_pane.attach_image(path)
    }

    pub(crate) fn attach_skill(&mut self, name: String, path: String) -> bool {
        self.input_pane.attach_skill(name, path)
    }

    pub(crate) fn set_available_skills(&mut self, skills: Vec<SkillMetadata>) {
        self.input_pane.set_available_skills(skills);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        mode: FrontendMode,
        status_indicator: Option<&str>,
        status_text: &str,
        runtime_hint: Option<&str>,
        status_meta: &str,
        hint_meta: &str,
    ) -> InputPaneRenderResult {
        self.input_pane.render(
            frame,
            area,
            mode,
            status_indicator,
            status_text,
            runtime_hint,
            status_meta,
            hint_meta,
        )
    }

    pub(crate) fn desired_height(&self, mode: FrontendMode, width: u16) -> u16 {
        self.input_pane.desired_height(mode, width)
    }

    pub(crate) fn clear_views(&mut self) {
        self.input_pane.clear_views();
        self.pending_session_picker = None;
    }

    pub(crate) fn show_transient_notice(&mut self, level: NoticeLevel, message: String) {
        self.runtime.show_transient_notice(level, message);
    }

    pub(crate) fn clear_composer(&mut self) {
        self.input_pane.clear_composer();
    }

    pub(crate) fn restore_submission(&mut self, content: &[InputItem]) {
        self.input_pane.restore_composer_submission(content);
    }

    pub(crate) fn set_server_request(&mut self, request: ServerRequestInlineState) {
        self.input_pane.set_server_request(request);
    }

    pub(crate) fn clear_server_request(&mut self) {
        self.input_pane.clear_server_request();
    }

    pub(crate) fn set_session_picker(
        &mut self,
        sessions: Vec<ConversationSummary>,
        active_conversation_id: &str,
        mode: SessionPickerMode,
    ) {
        self.input_pane
            .set_session_picker(sessions, active_conversation_id, mode);
    }

    pub(crate) fn clear_session_picker(&mut self) {
        self.input_pane.clear_session_picker();
        self.pending_session_picker = None;
    }

    pub(crate) fn set_filter_picker(&mut self) {
        self.input_pane.set_filter_picker();
    }

    pub(crate) fn set_help_view(&mut self) {
        self.input_pane.set_help_view();
    }

    pub(crate) fn request_session_picker(&mut self, mode: SessionPickerMode) {
        self.pending_session_picker = Some(mode);
    }

    pub(crate) fn present_requested_session_picker(
        &mut self,
        sessions: Vec<ConversationSummary>,
        active_conversation_id: &str,
    ) -> bool {
        let Some(mode) = self.pending_session_picker.take() else {
            return false;
        };
        self.set_session_picker(sessions, active_conversation_id, mode);
        true
    }

    pub(crate) fn set_permissions_picker(&mut self, current: &str) {
        self.input_pane.set_permissions_picker(current);
    }

    pub(crate) fn set_reasoning_picker(&mut self, current: ReasoningEffort) {
        self.input_pane.set_reasoning_picker(current);
    }

    pub(crate) fn set_model_picker(&mut self, current: String, models: Vec<String>) {
        self.input_pane.set_model_picker(current, models);
    }

    pub(crate) fn set_config_panel(&mut self, api_key: String, base_url: String, model: String) {
        self.input_pane.set_config_panel(api_key, base_url, model);
    }

    pub(crate) fn set_gateway_list_panel(&mut self, entries: Vec<PlatformControlEntry>) {
        self.input_pane.set_gateway_list_panel(entries);
    }

    pub(crate) fn set_gateway_edit_panel(
        &mut self,
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
    ) {
        self.input_pane.set_gateway_edit_panel(entry, config);
    }

    pub(crate) fn set_gateway_edit_panel_with_weixin_login(
        &mut self,
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
        session: Option<WeixinLoginSessionView>,
    ) {
        self.input_pane
            .set_gateway_edit_panel_with_weixin_login(entry, config, session);
    }

    pub(crate) fn set_weixin_binding_view(&mut self, model: WeixinBindingViewModel) {
        self.input_pane.set_weixin_binding_view(model);
    }

    pub(crate) fn dismiss_server_request(&mut self, request_id: &RequestId) {
        self.input_pane.dismiss_server_request(request_id);
    }

    pub(crate) fn build_status_view_model(&self, app: &TuiApp) -> StatusViewModel {
        let mode = app.current_mode();
        let fallback = status_text_from_mode(mode);
        let (live_banner, live_banner_level) = self.runtime_banner_text();
        let text = fallback.to_string();
        let indicator = match mode {
            FrontendMode::Running | FrontendMode::WaitingForServerRequest => {
                Some(animated_indicator(app.run_state.live_animation_frame).to_string())
            }
            FrontendMode::Idle => None,
        };
        let runtime_hint = self
            .runtime
            .turn_started_at
            .map(|started| fmt_elapsed_compact(started.elapsed().as_secs()));

        let mut parts = Vec::new();
        let hint_meta = format!(
            "filter {} · perm {}",
            if app.run_state.pre_llm_filter_enabled {
                "on"
            } else {
                "off"
            },
            app.run_state.permission_mode
        );
        if let Some(usage) = &app.run_state.last_turn_usage {
            parts.push(format!(
                "in {} · out {} · cached {} · total {}",
                format_tokens(usage.input_tokens),
                format_tokens(usage.total_output_tokens()),
                format_tokens(usage.cached_input_tokens),
                format_tokens(usage.total_consumed_tokens())
            ));
        }
        if let (Some(last), Some(window)) = (
            &app.run_state.last_turn_usage,
            app.run_state.model_context_window,
        ) && window > 0
        {
            let percent = last.total_consumed_tokens().saturating_mul(100) / window;
            parts.push(format!("context {percent}%"));
        }
        StatusViewModel {
            indicator,
            text,
            runtime_hint,
            meta: parts.join(" · "),
            hint_meta,
            live_banner,
            live_banner_level,
        }
    }

    fn runtime_banner_text(&self) -> (Option<String>, Option<NoticeLevel>) {
        if let Some(notice) = self.runtime.transient_notice.as_ref() {
            return (Some(notice.message.clone()), Some(notice.level));
        }
        if let Some(command) = self.runtime.active_command.as_ref() {
            return (Some(command.banner_text()), None);
        }
        if let Some(tool_title) = self.runtime.active_tool_title.as_deref() {
            return (Some(tool_title.to_string()), None);
        }
        let Some(live_label) = self.runtime.live_label.as_deref() else {
            return (None, None);
        };
        let live_label = live_label.trim();
        if live_label.is_empty() || live_label.eq_ignore_ascii_case("working") {
            return (None, None);
        }
        (Some(live_label.to_string()), None)
    }

    #[cfg(test)]
    pub(crate) fn live_label_override_for_test(&mut self, label: Option<String>) {
        self.runtime.set_live_label_for_test(label);
    }

    #[cfg(test)]
    pub(crate) fn active_tool_title_override_for_test(&mut self, title: Option<String>) {
        self.runtime.set_active_tool_title_for_test(title);
    }

    #[cfg(test)]
    pub(crate) fn expire_transient_notice_for_test(&mut self) {
        self.runtime.expire_transient_notice_for_test();
    }

    #[cfg(test)]
    pub(crate) fn render_lines_for_test(
        &self,
        mode: FrontendMode,
        status_text: &str,
        status_meta: &str,
        area_width: u16,
    ) -> (Vec<ratatui::text::Line<'static>>, u16) {
        self.input_pane
            .render_lines_for_test(mode, status_text, status_meta, area_width)
    }
}

fn animated_indicator(frame: u64) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(frame as usize) % FRAMES.len()]
}

fn fmt_elapsed_compact(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        return format!("{elapsed_secs}s");
    }
    if elapsed_secs < 3600 {
        let minutes = elapsed_secs / 60;
        let seconds = elapsed_secs % 60;
        return format!("{minutes}m {seconds:02}s");
    }
    let hours = elapsed_secs / 3600;
    let minutes = (elapsed_secs % 3600) / 60;
    let seconds = elapsed_secs % 60;
    format!("{hours}h {minutes:02}m {seconds:02}s")
}

fn compact_number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn format_tokens(value: u64) -> String {
    format!("{} tokens", compact_number(value))
}

#[cfg(test)]
mod tests {
    use crate::app::TuiApp;
    use crate::state::NoticeLevel;
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

    fn mark_running(app: &mut TuiApp) {
        app.apply_conversation_view_snapshot(running_snapshot(&app.conversation_id));
        app.bottom_pane.on_turn_started();
    }

    fn running_snapshot(conversation_id: &str) -> agent_protocol::ConversationViewSnapshot {
        agent_protocol::ConversationViewSnapshot {
            conversation_id: conversation_id.to_string(),
            status: agent_protocol::ConversationViewStatus::Active {
                active_turn_id: None,
                flags: vec![agent_protocol::ConversationActiveFlag::RunningTurn],
            },
            active_turn: None,
            pending_requests: Vec::new(),
            message_count: 0,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn active_tool_status_overrides_live_label() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 1;
        mark_running(&mut app);
        app.bottom_pane
            .live_label_override_for_test(Some("Working".to_string()));
        app.bottom_pane
            .active_tool_title_override_for_test(Some("running command: rg cli".to_string()));

        let status = app.bottom_pane.build_status_view_model(&app);

        assert_eq!(status.text, "Working");
        assert_eq!(status.indicator.as_deref(), Some("⠙"));
        assert_eq!(
            status.live_banner.as_deref(),
            Some("running command: rg cli")
        );
        assert_eq!(status.runtime_hint.as_deref(), Some("0s"));
    }

    #[test]
    fn command_output_delta_stays_in_runtime_banner() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 1;
        mark_running(&mut app);
        app.bottom_pane.on_active_item_started(
            "cmd-1",
            &agent_core::TurnItemKind::CommandExecution,
            Some("rg TODO"),
        );
        app.bottom_pane
            .on_command_output_delta(Some("cmd-1"), "src/main.rs:12: TODO clean this up\n");

        let status = app.bottom_pane.build_status_view_model(&app);

        assert_eq!(
            status.live_banner.as_deref(),
            Some("running command: rg TODO · src/main.rs:12: TODO clean this up")
        );

        app.bottom_pane.on_command_finished("cmd-1");
        let after = app.bottom_pane.build_status_view_model(&app);
        assert_eq!(after.live_banner, None);
    }

    #[test]
    fn command_output_delta_keeps_recent_tail_compact() {
        let mut app = test_app();
        mark_running(&mut app);
        app.bottom_pane.on_active_item_started(
            "cmd-1",
            &agent_core::TurnItemKind::CommandExecution,
            Some("long command"),
        );

        app.bottom_pane
            .on_command_output_delta(Some("cmd-1"), &"alpha ".repeat(80));
        app.bottom_pane
            .on_command_output_delta(Some("cmd-1"), "omega");

        let status = app.bottom_pane.build_status_view_model(&app);
        let banner = status.live_banner.expect("command banner");
        assert!(banner.starts_with("running command: long command · …"));
        assert!(banner.ends_with("omega"));
        assert!(banner.chars().count() <= "running command: long command · ".chars().count() + 121);
    }

    #[test]
    fn stale_command_output_delta_does_not_update_current_banner() {
        let mut app = test_app();
        mark_running(&mut app);
        app.bottom_pane.on_active_item_started(
            "cmd-current",
            &agent_core::TurnItemKind::CommandExecution,
            Some("cargo check"),
        );

        app.bottom_pane
            .on_command_output_delta(Some("cmd-old"), "old command output");

        let status = app.bottom_pane.build_status_view_model(&app);
        assert_eq!(
            status.live_banner.as_deref(),
            Some("running command: cargo check")
        );
    }

    #[test]
    fn stale_command_finish_does_not_clear_current_banner() {
        let mut app = test_app();
        mark_running(&mut app);
        app.bottom_pane.on_active_item_started(
            "cmd-current",
            &agent_core::TurnItemKind::CommandExecution,
            Some("cargo test"),
        );

        app.bottom_pane.on_command_finished("cmd-old");

        let status = app.bottom_pane.build_status_view_model(&app);
        assert_eq!(
            status.live_banner.as_deref(),
            Some("running command: cargo test")
        );
    }

    #[test]
    fn in_progress_completion_keeps_command_runtime_until_final_completion() {
        let mut app = test_app();
        mark_running(&mut app);
        app.bottom_pane.on_active_item_started(
            "cmd-1",
            &agent_core::TurnItemKind::CommandExecution,
            Some("slow command"),
        );

        let status = app.bottom_pane.build_status_view_model(&app);
        assert_eq!(
            status.live_banner.as_deref(),
            Some("running command: slow command")
        );

        app.bottom_pane
            .on_command_output_delta(Some("cmd-1"), "still running");
        let status = app.bottom_pane.build_status_view_model(&app);
        assert_eq!(
            status.live_banner.as_deref(),
            Some("running command: slow command · still running")
        );
    }

    #[test]
    fn reconnect_live_label_animates_when_no_active_tool_or_notice() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 2;
        mark_running(&mut app);
        app.bottom_pane.live_label_override_for_test(Some(
            "reconnecting (stream retry 2, next in 1.0s)".to_string(),
        ));

        let status = app.bottom_pane.build_status_view_model(&app);

        assert_eq!(status.text, "Working");
        assert_eq!(status.indicator.as_deref(), Some("⠹"));
        assert_eq!(
            status.live_banner.as_deref(),
            Some("reconnecting (stream retry 2, next in 1.0s)")
        );
        assert_eq!(status.runtime_hint.as_deref(), Some("0s"));
    }

    #[test]
    fn generic_live_label_hides_when_active_cell_is_visible() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 0;
        mark_running(&mut app);
        app.bottom_pane
            .live_label_override_for_test(Some("Thinking".to_string()));
        app.transcript_owner.push_live_cell(
            crate::ui::widgets::history_cell::HistoryCell::reasoning("Reasoning", "streaming body"),
        );

        let status = app.bottom_pane.build_status_view_model(&app);

        assert_eq!(status.text, "Working");
        assert_eq!(status.live_banner.as_deref(), Some("Thinking"));
        assert_eq!(status.runtime_hint.as_deref(), Some("0s"));
    }

    #[test]
    fn generic_live_label_does_not_render_external_banner_without_active_cell() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 0;
        mark_running(&mut app);
        app.bottom_pane
            .live_label_override_for_test(Some("Thinking".to_string()));

        let status = app.bottom_pane.build_status_view_model(&app);

        assert_eq!(status.text, "Working");
        assert_eq!(status.live_banner.as_deref(), Some("Thinking"));
        assert_eq!(status.runtime_hint.as_deref(), Some("0s"));
    }

    #[test]
    fn working_without_runtime_does_not_show_elapsed_hint() {
        let mut app = test_app();
        app.apply_conversation_view_snapshot(running_snapshot(&app.conversation_id));
        app.bottom_pane
            .live_label_override_for_test(Some("Working".to_string()));

        let status = app.bottom_pane.build_status_view_model(&app);

        assert_eq!(status.text, "Working");
        assert_eq!(status.runtime_hint, None);
        assert_eq!(status.live_banner, None);
    }

    #[test]
    fn compaction_runtime_status_renders_as_live_banner_and_clears_cleanly() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 3;
        mark_running(&mut app);
        app.bottom_pane.on_context_compaction_started(12_345);

        let during = app.bottom_pane.build_status_view_model(&app);
        assert_eq!(during.text, "Working");
        assert_eq!(during.indicator.as_deref(), Some("⠸"));
        assert_eq!(
            during.live_banner.as_deref(),
            Some("Compacting context (~12.3k tokens)")
        );

        app.bottom_pane.on_context_compaction_finished();
        let after = app.bottom_pane.build_status_view_model(&app);
        assert_eq!(after.text, "Working");
        assert_eq!(after.live_banner, None);
    }

    #[test]
    fn transient_notice_renders_above_runtime_banner_and_expires() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 1;
        mark_running(&mut app);
        app.bottom_pane
            .active_tool_title_override_for_test(Some("running command: rg cli".to_string()));
        app.bottom_pane.show_transient_notice(
            NoticeLevel::Info,
            "Deleted conversation `draft-1`".to_string(),
        );

        let during = app.bottom_pane.build_status_view_model(&app);
        assert_eq!(
            during.live_banner.as_deref(),
            Some("Deleted conversation `draft-1`")
        );
        assert_eq!(during.live_banner_level, Some(NoticeLevel::Info));

        app.bottom_pane.expire_transient_notice_for_test();
        assert!(app.bottom_pane.handle_tick());

        let after = app.bottom_pane.build_status_view_model(&app);
        assert_eq!(
            after.live_banner.as_deref(),
            Some("running command: rg cli")
        );
        assert_eq!(after.live_banner_level, None);
    }
}
