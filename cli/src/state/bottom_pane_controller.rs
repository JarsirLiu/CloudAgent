use crate::app::TuiApp;
use crate::state::bottom_pane_runtime::BottomPaneRuntimeState;
use crate::state::selectors::status_text_from_mode;
use crate::terminal::Frame;
use crate::ui::widgets::input_pane::{
    InputPane, InputPaneAction, InputPaneRenderResult, ServerRequestInlineState,
};
use crate::ui::widgets::session_picker::SessionPickerMode;
use agent_core::{ConversationSummary, ModelRetryStage, TurnItemKind};
use agent_core::InputItem;
use agent_protocol::FrontendMode;
use agent_protocol::RequestId;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use std::path::PathBuf;

pub(crate) struct StatusViewModel {
    pub(crate) indicator: Option<String>,
    pub(crate) text: String,
    pub(crate) runtime_hint: Option<String>,
    pub(crate) meta: String,
    pub(crate) hint_meta: String,
    pub(crate) live_banner: Option<String>,
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

    pub(crate) fn on_active_item_started(&mut self, kind: &TurnItemKind, title: Option<&str>) {
        self.runtime.on_active_item_started(kind, title);
    }

    pub(crate) fn prepare_for_submit(&mut self) {
        self.clear_views();
        self.clear_composer();
        self.on_turn_started();
    }

    pub(crate) fn derive_mode(
        &self,
        requires_action: bool,
        has_active_turn: bool,
        _has_live_cell: bool,
    ) -> FrontendMode {
        let has_runtime_activity = self.runtime.turn_active
            || self.runtime.live_label.is_some()
            || self.runtime.active_tool_title.is_some();
        if requires_action {
            FrontendMode::WaitingForServerRequest
        } else if has_runtime_activity || has_active_turn {
            FrontendMode::Running
        } else {
            FrontendMode::Idle
        }
    }

    pub(crate) fn current_mode(&self, has_active_turn: bool, has_live_cell: bool) -> FrontendMode {
        self.derive_mode(self.requires_action(), has_active_turn, has_live_cell)
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<InputPaneAction> {
        self.input_pane.handle_key(key)
    }

    pub(crate) fn handle_paste(&mut self, text: &str) -> Option<InputPaneAction> {
        self.input_pane.handle_paste(text)
    }

    pub(crate) fn handle_tick(&mut self) -> bool {
        self.input_pane.handle_tick()
    }

    pub(crate) fn composer_has_selection(&self) -> bool {
        self.input_pane.composer_has_selection()
    }

    pub(crate) fn composer_is_empty(&self) -> bool {
        self.input_pane.composer_is_empty()
    }

    pub(crate) fn attach_image(&mut self, path: PathBuf) -> bool {
        self.input_pane.attach_image(path)
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

    pub(crate) fn set_config_panel(&mut self, api_key: String, base_url: String, model: String) {
        self.input_pane.set_config_panel(api_key, base_url, model);
    }

    pub(crate) fn dismiss_server_request(&mut self, request_id: &RequestId) {
        self.input_pane.dismiss_server_request(request_id);
    }

    pub(crate) fn requires_action(&self) -> bool {
        self.input_pane.requires_action()
    }

    pub(crate) fn build_status_view_model(&self, app: &TuiApp) -> StatusViewModel {
        let mode = app.current_mode();
        let fallback = status_text_from_mode(mode);
        let live_banner = self.runtime_banner_text();
        let text = fallback.to_string();
        let indicator = match mode {
            FrontendMode::Running | FrontendMode::WaitingForServerRequest => {
                Some(animated_indicator(app.run_state.live_animation_frame).to_string())
            }
            FrontendMode::Idle => None,
        };
        let runtime_hint = self.runtime.turn_started_at.map(|started| {
            format!(
                "{} • esc to interrupt",
                fmt_elapsed_compact(started.elapsed().as_secs())
            )
        });

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
        }
    }

    fn runtime_banner_text(&self) -> Option<String> {
        if let Some(tool_title) = self.runtime.active_tool_title.as_deref() {
            return Some(tool_title.to_string());
        }
        let live_label = self.runtime.live_label.as_deref()?.trim();
        if live_label.is_empty() || live_label.eq_ignore_ascii_case("working") {
            return None;
        }
        Some(live_label.to_string())
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
    fn active_tool_status_overrides_live_label() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 1;
        app.bottom_pane.on_turn_started();
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
        assert_eq!(
            status.runtime_hint.as_deref(),
            Some("0s • esc to interrupt")
        );
    }

    #[test]
    fn reconnect_live_label_animates_when_no_active_tool_or_notice() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 2;
        app.bottom_pane.on_turn_started();
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
        assert_eq!(
            status.runtime_hint.as_deref(),
            Some("0s • esc to interrupt")
        );
    }

    #[test]
    fn generic_live_label_hides_when_active_cell_is_visible() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 0;
        app.bottom_pane.on_turn_started();
        app.bottom_pane
            .live_label_override_for_test(Some("Thinking".to_string()));
        app.transcript_owner.push_live_cell(
            crate::ui::widgets::history_cell::HistoryCell::reasoning("Reasoning", "streaming body"),
        );

        let status = app.bottom_pane.build_status_view_model(&app);

        assert_eq!(status.text, "Working");
        assert_eq!(status.live_banner.as_deref(), Some("Thinking"));
        assert_eq!(
            status.runtime_hint.as_deref(),
            Some("0s • esc to interrupt")
        );
    }

    #[test]
    fn generic_live_label_does_not_render_external_banner_without_active_cell() {
        let mut app = test_app();
        app.run_state.live_animation_frame = 0;
        app.bottom_pane.on_turn_started();
        app.bottom_pane
            .live_label_override_for_test(Some("Thinking".to_string()));

        let status = app.bottom_pane.build_status_view_model(&app);

        assert_eq!(status.text, "Working");
        assert_eq!(status.live_banner.as_deref(), Some("Thinking"));
        assert_eq!(
            status.runtime_hint.as_deref(),
            Some("0s • esc to interrupt")
        );
    }

    #[test]
    fn working_without_runtime_does_not_show_elapsed_hint() {
        let mut app = test_app();
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
        app.bottom_pane.on_turn_started();
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
}
