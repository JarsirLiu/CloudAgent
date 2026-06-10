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
    pending_session_picker: Option<PendingSessionPicker>,
    next_session_picker_generation: u64,
}

#[derive(Clone, Copy)]
struct PendingSessionPicker {
    mode: SessionPickerMode,
    generation: u64,
}

impl BottomPaneController {
    pub(crate) fn new() -> Self {
        Self {
            runtime: BottomPaneRuntimeState::default(),
            input_pane: InputPane::new(),
            pending_session_picker: None,
            next_session_picker_generation: 1,
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

    pub(crate) fn no_modal_or_popup_active(&self) -> bool {
        self.input_pane.no_modal_or_popup_active()
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

    pub(crate) fn append_session_page(
        &mut self,
        sessions: Vec<ConversationSummary>,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> bool {
        self.input_pane
            .append_session_page(sessions, has_more, next_cursor)
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
        let generation = self.next_session_picker_generation;
        self.next_session_picker_generation = self.next_session_picker_generation.saturating_add(1);
        self.pending_session_picker = Some(PendingSessionPicker { mode, generation });
        self.input_pane.set_session_picker_loading(generation, mode);
    }

    pub(crate) fn present_requested_session_picker(
        &mut self,
        sessions: Vec<ConversationSummary>,
        active_conversation_id: &str,
    ) -> bool {
        let Some(pending) = self.pending_session_picker.take() else {
            return false;
        };
        if !self
            .input_pane
            .is_session_picker_loading(pending.generation)
        {
            return false;
        }
        self.set_session_picker(sessions, active_conversation_id, pending.mode);
        true
    }

    pub(crate) fn present_requested_session_picker_page(
        &mut self,
        sessions: Vec<ConversationSummary>,
        active_conversation_id: &str,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> bool {
        let Some(pending) = self.pending_session_picker.take() else {
            return false;
        };
        if !self
            .input_pane
            .is_session_picker_loading(pending.generation)
        {
            return false;
        }
        self.input_pane.set_session_picker_page(
            sessions,
            active_conversation_id,
            pending.mode,
            has_more,
            next_cursor,
        );
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

    pub(crate) fn set_model_picker_loading(&mut self, current: String) {
        self.input_pane.set_model_picker_loading(current);
    }

    pub(crate) fn is_model_picker_loading(&self) -> bool {
        self.input_pane.is_model_picker_loading()
    }

    pub(crate) fn set_config_panel(&mut self, api_key: String, base_url: String, model: String) {
        self.input_pane.set_config_panel(api_key, base_url, model);
    }

    pub(crate) fn set_gateway_list_panel(&mut self, entries: Vec<PlatformControlEntry>) {
        self.input_pane.set_gateway_list_panel(entries);
    }

    pub(crate) fn push_gateway_edit_panel(
        &mut self,
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
    ) {
        self.input_pane.push_gateway_edit_panel(entry, config);
    }

    pub(crate) fn replace_gateway_edit_panel(
        &mut self,
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
    ) {
        self.input_pane.replace_gateway_edit_panel(entry, config);
    }

    pub(crate) fn replace_parent_with_gateway_edit_panel(
        &mut self,
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
    ) {
        self.input_pane
            .replace_parent_with_gateway_edit_panel(entry, config);
    }

    pub(crate) fn replace_gateway_edit_panel_with_weixin_login(
        &mut self,
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
        session: Option<WeixinLoginSessionView>,
    ) {
        self.input_pane
            .replace_gateway_edit_panel_with_weixin_login(entry, config, session);
    }

    pub(crate) fn push_weixin_binding_view(&mut self, model: WeixinBindingViewModel) {
        self.input_pane.push_weixin_binding_view(model);
    }

    pub(crate) fn replace_weixin_binding_view(&mut self, model: WeixinBindingViewModel) {
        self.input_pane.replace_weixin_binding_view(model);
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
mod tests;
