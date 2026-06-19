use super::InputPane;
use crate::ui::bottom_pane::bottom_pane_view::ViewKind;
use crate::ui::bottom_pane::dialogs::config_panel::ConfigPanel;
use crate::ui::bottom_pane::dialogs::gateway_panel::{GatewayPanel, WeixinLoginSessionView};
use crate::ui::bottom_pane::dialogs::help_view::HelpView;
use crate::ui::bottom_pane::dialogs::selection::filter_picker::FilterPicker;
use crate::ui::bottom_pane::dialogs::selection::model_picker::ModelPicker;
use crate::ui::bottom_pane::dialogs::selection::model_picker_loading::ModelPickerLoading;
use crate::ui::bottom_pane::dialogs::selection::permissions_picker::PermissionsPicker;
use crate::ui::bottom_pane::dialogs::selection::reasoning_picker::ReasoningPicker;
use crate::ui::bottom_pane::dialogs::selection::session_picker::{
    SessionPicker, SessionPickerMode,
};
use crate::ui::bottom_pane::dialogs::selection::session_picker_loading::SessionPickerLoading;
use crate::ui::bottom_pane::dialogs::server_request::server_request_overlay::ServerRequestOverlay;
use crate::ui::bottom_pane::dialogs::weixin_binding_view::{
    WeixinBindingView, WeixinBindingViewModel,
};
use agent_core::ConversationSummary;
use agent_core::InputItem;
use agent_protocol::{PlatformConfigResponse, PlatformControlEntry, RequestId};
use config::ReasoningEffort;

impl InputPane {
    pub fn clear_views(&mut self) {
        self.navigator.clear();
    }

    pub fn clear_composer(&mut self) {
        self.composer.clear();
    }

    pub fn restore_composer_submission(&mut self, content: &[InputItem]) {
        self.navigator.clear();
        self.composer.restore_submission(content);
    }

    pub(crate) fn set_server_request(&mut self, request: super::ServerRequestInlineState) {
        let request = if let Some(view) = self.navigator.active_view_mut() {
            match view.try_consume_server_request(request) {
                Some(request) => request,
                None => return,
            }
        } else {
            request
        };

        self.navigator
            .replace(Box::new(ServerRequestOverlay::new(request)));
    }

    pub fn clear_server_request(&mut self) {
        self.navigator.clear();
    }

    pub fn set_session_picker(
        &mut self,
        sessions: Vec<ConversationSummary>,
        active_conversation_id: &str,
        mode: SessionPickerMode,
    ) {
        self.navigator.replace(Box::new(SessionPicker::new(
            sessions,
            active_conversation_id,
            mode,
        )));
    }

    pub fn set_session_picker_page(
        &mut self,
        sessions: Vec<ConversationSummary>,
        active_conversation_id: &str,
        mode: SessionPickerMode,
        has_more: bool,
        next_cursor: Option<String>,
    ) {
        self.navigator.replace(Box::new(SessionPicker::new_page(
            sessions,
            active_conversation_id,
            mode,
            has_more,
            next_cursor,
        )));
    }

    pub fn append_session_page(
        &mut self,
        sessions: Vec<ConversationSummary>,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> bool {
        self.navigator
            .active_view_mut()
            .is_some_and(|view| view.append_session_page(sessions, has_more, next_cursor))
    }

    pub fn clear_session_picker(&mut self) {
        self.navigator.retain(|view| {
            !matches!(
                view.kind(),
                ViewKind::SessionPicker | ViewKind::SessionPickerLoading
            )
        });
    }

    pub fn set_session_picker_loading(&mut self, mode: SessionPickerMode) {
        self.navigator
            .replace(Box::new(SessionPickerLoading::new(mode)));
    }

    pub fn set_filter_picker(&mut self) {
        self.navigator.replace(Box::new(FilterPicker::new()));
    }

    pub fn set_help_view(&mut self) {
        self.navigator.replace(Box::new(HelpView::new()));
    }

    pub fn set_permissions_picker(&mut self, current: &str) {
        self.navigator
            .replace(Box::new(PermissionsPicker::new(current)));
    }

    pub fn set_reasoning_picker(&mut self, current: ReasoningEffort) {
        self.navigator
            .replace(Box::new(ReasoningPicker::new(current)));
    }

    pub fn set_model_picker(&mut self, current: String, models: Vec<String>) {
        self.navigator
            .replace(Box::new(ModelPicker::new(current, models)));
    }

    pub fn set_model_picker_loading(&mut self, current: String) {
        self.navigator
            .replace(Box::new(ModelPickerLoading::new(current)));
    }

    pub fn set_config_panel(&mut self, api_key: String, base_url: String, model: String) {
        self.navigator
            .replace(Box::new(ConfigPanel::new(api_key, base_url, model)));
    }

    pub fn set_gateway_list_panel(&mut self, entries: Vec<PlatformControlEntry>) {
        self.navigator
            .replace(Box::new(GatewayPanel::list(entries)));
    }

    pub fn push_gateway_edit_panel(
        &mut self,
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
    ) {
        self.navigator
            .push(Box::new(GatewayPanel::edit(entry, config, None)));
    }

    pub fn replace_gateway_edit_panel(
        &mut self,
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
    ) {
        self.navigator
            .replace_active(Box::new(GatewayPanel::edit(entry, config, None)));
    }

    pub fn replace_parent_with_gateway_edit_panel(
        &mut self,
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
    ) {
        self.navigator
            .replace_parent_after_child(Box::new(GatewayPanel::edit(entry, config, None)));
    }

    pub fn replace_gateway_edit_panel_with_weixin_login(
        &mut self,
        entry: PlatformControlEntry,
        config: PlatformConfigResponse,
        session: Option<WeixinLoginSessionView>,
    ) {
        self.navigator
            .replace_active(Box::new(GatewayPanel::edit(entry, config, session)));
    }

    pub fn push_weixin_binding_view(&mut self, model: WeixinBindingViewModel) {
        self.navigator.push(Box::new(WeixinBindingView::new(model)));
    }

    pub fn replace_weixin_binding_view(&mut self, model: WeixinBindingViewModel) {
        self.navigator
            .replace_active(Box::new(WeixinBindingView::new(model)));
    }

    pub fn dismiss_server_request(&mut self, request_id: &RequestId) {
        self.navigator.dismiss_server_request(request_id);
    }

    pub fn active_server_request_id(&self) -> Option<RequestId> {
        self.navigator
            .active_view()
            .and_then(|view| view.active_server_request_id().cloned())
    }
}
