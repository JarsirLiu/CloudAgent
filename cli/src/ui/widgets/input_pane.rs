use crate::input::intent::ComposerIntent;
use crate::terminal::Frame;
use crate::ui::bottom_pane_navigation::{BottomPaneNavigator, NavigationKeyResult};
use crate::ui::widgets::bottom_pane_view::BottomPaneViewAction;
use crate::ui::widgets::chat_composer::ChatComposer;
use crate::ui::widgets::config_panel::ConfigPanel;
use crate::ui::widgets::filter_picker::FilterPicker;
use crate::ui::widgets::footer::{hint_line, status_line};
use crate::ui::widgets::gateway_panel::{GatewayPanel, WeixinLoginSessionView};
use crate::ui::widgets::help_view::HelpView;
use crate::ui::widgets::model_picker::ModelPicker;
use crate::ui::widgets::model_picker_loading::ModelPickerLoading;
use crate::ui::widgets::permissions_picker::PermissionsPicker;
use crate::ui::widgets::reasoning_picker::ReasoningPicker;
pub(crate) use crate::ui::widgets::server_request_model::ServerRequestInlineState;
use crate::ui::widgets::server_request_overlay::ServerRequestOverlay;
use crate::ui::widgets::session_picker::{SessionPicker, SessionPickerMode};
use crate::ui::widgets::session_picker_loading::SessionPickerLoading;
use crate::ui::widgets::weixin_binding_view::{WeixinBindingView, WeixinBindingViewModel};
use agent_core::ConversationSummary;
use agent_core::InputItem;
use agent_core::ServerRequestDecisionKind;
use agent_core::SkillMetadata;
use agent_protocol::{FrontendMode, PlatformConfigResponse, PlatformControlEntry, RequestId};
use config::ReasoningEffort;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use std::path::PathBuf;
use std::time::Duration;

pub struct InputPane {
    composer: ChatComposer,
    navigator: BottomPaneNavigator,
}

pub(crate) enum InputPaneAction {
    Composer(ComposerIntent),
    LoadMoreSessions {
        cursor: String,
    },
    ServerRequestSubmit {
        request_id: RequestId,
        decision: ServerRequestDecisionKind,
        reason: String,
    },
}

pub(crate) struct InputPaneRenderResult {
    pub cursor_position: Option<(u16, u16)>,
}

#[derive(Clone, Copy)]
struct InputPaneLayout {
    input_area: Rect,
    composer_area: Rect,
    completion_area: Option<Rect>,
}

struct InputPaneSnapshot {
    layout: InputPaneLayout,
    input_lines: Vec<Line<'static>>,
    completion_lines: Vec<Line<'static>>,
    cursor_position: Option<(u16, u16)>,
    height: u16,
}

const STATUS_ROW_HEIGHT: u16 = 1;
const COMPOSER_TOP_SPACER_HEIGHT: u16 = 1;
const COMPOSER_BOTTOM_SPACER_HEIGHT: u16 = 0;
const HINT_ROW_HEIGHT: u16 = 1;
const INPUT_BLOCK_CHROME_HEIGHT: u16 = 2;

impl InputPane {
    pub fn new() -> Self {
        Self {
            composer: ChatComposer::new(),
            navigator: BottomPaneNavigator::new(),
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<InputPaneAction> {
        match self.navigator.handle_key(key) {
            NavigationKeyResult::NoActiveView => {}
            NavigationKeyResult::Consumed => {
                return Some(InputPaneAction::Composer(ComposerIntent::None));
            }
            NavigationKeyResult::Composer(intent) => {
                return Some(InputPaneAction::Composer(intent));
            }
            NavigationKeyResult::LoadMoreSessions { cursor } => {
                return Some(InputPaneAction::LoadMoreSessions { cursor });
            }
            NavigationKeyResult::ServerRequestSubmit {
                request_id,
                decision,
                reason,
            } => {
                return Some(InputPaneAction::ServerRequestSubmit {
                    request_id,
                    decision,
                    reason,
                });
            }
            NavigationKeyResult::FallthroughEscFromActionRequiredView => {}
        }

        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            return self.handle_escape_key();
        }

        self.composer.handle_key(key).map(InputPaneAction::Composer)
    }

    fn handle_escape_key(&mut self) -> Option<InputPaneAction> {
        // Completion/menu Esc is a navigation action, not an interrupt.
        if self.composer.has_completion_menu() {
            return self
                .composer
                .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .map(InputPaneAction::Composer);
        }
        match self
            .composer
            .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        {
            Some(action) => Some(InputPaneAction::Composer(action)),
            None => Some(InputPaneAction::Composer(ComposerIntent::Interrupt)),
        }
    }

    pub(crate) fn handle_paste(&mut self, text: &str) -> Option<InputPaneAction> {
        if let Some(action) = self.navigator.handle_paste(text) {
            return match action {
                BottomPaneViewAction::Composer(intent) => Some(InputPaneAction::Composer(intent)),
                _ => None,
            };
        }
        Some(InputPaneAction::Composer(self.composer.handle_paste(text)))
    }

    pub(crate) fn composer_has_selection(&self) -> bool {
        self.navigator.is_empty() && self.composer.has_selection()
    }

    pub(crate) fn should_capture_global_paste_shortcut(&self) -> bool {
        if let Some(view) = self.navigator.active_view() {
            view.should_capture_global_paste_shortcut()
        } else {
            true
        }
    }

    pub(crate) fn attach_image(&mut self, path: PathBuf) -> bool {
        if self.navigator.is_empty() {
            self.composer.attach_image(path);
            true
        } else {
            false
        }
    }

    pub(crate) fn attach_skill(&mut self, name: String, path: String) -> bool {
        if self.navigator.is_empty() {
            self.composer.attach_skill(name, path);
            true
        } else {
            false
        }
    }

    pub(crate) fn set_available_skills(&mut self, skills: Vec<SkillMetadata>) {
        let skills = skills
            .into_iter()
            .map(|skill| crate::input::completion::SkillCompletion {
                name: skill.name,
                description: skill.description,
                path: skill.path.display().to_string(),
            })
            .collect();
        self.composer.set_available_skills(skills);
    }

    pub(crate) fn handle_tick(&mut self) -> bool {
        self.navigator.is_empty() && self.composer.flush_paste_burst_if_due()
    }

    pub(crate) fn next_paste_flush_delay(&self) -> Option<Duration> {
        if self.navigator.is_empty() {
            self.composer.next_paste_flush_delay()
        } else {
            None
        }
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
        if self
            .navigator
            .active_view()
            .is_some_and(|view| view.requires_action())
        {
            let (widget, lines_before_composer, _) = self.render_request_view(
                mode,
                status_indicator,
                status_text,
                runtime_hint,
                status_meta,
                area.width,
            );
            frame.render_widget(widget, area);
            return InputPaneRenderResult {
                cursor_position: self.cursor_position(area, lines_before_composer, mode),
            };
        }

        let inner_width = area.width.saturating_sub(2) as usize;
        let snapshot = self.build_snapshot(
            area,
            mode,
            status_indicator,
            status_text,
            runtime_hint,
            status_meta,
            hint_meta,
            inner_width,
        );
        frame.render_widget(
            input_block(snapshot.input_lines, border_style(mode)),
            snapshot.layout.input_area,
        );

        if let Some(completion_area) = snapshot.layout.completion_area {
            let panel = Paragraph::new(Text::from(snapshot.completion_lines)).block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::Rgb(58, 64, 86))),
            );
            frame.render_widget(panel, completion_area);
        }

        InputPaneRenderResult {
            cursor_position: snapshot.cursor_position,
        }
    }

    fn render_request_view(
        &self,
        mode: FrontendMode,
        status_indicator: Option<&str>,
        status_text: &str,
        runtime_hint: Option<&str>,
        _status_meta: &str,
        area_width: u16,
    ) -> (Paragraph<'static>, u16, u16) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let inner_width = area_width.saturating_sub(2) as usize;
        lines.push(status_line(
            mode,
            status_indicator,
            status_text,
            runtime_hint,
            "",
            inner_width,
        ));

        let mut lines_before_composer = 1u16;

        if let Some(view) = self.navigator.active_view() {
            lines.push(Line::raw(""));
            lines_before_composer += 1;
            let view_lines = view.render_lines(area_width.saturating_sub(2));
            lines_before_composer += view_lines.len() as u16;
            lines.extend(view_lines);
        }

        let total_lines = lines.len() as u16;
        (
            Paragraph::new(Text::from(lines)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border_style(mode))
                    .title_style(
                        Style::default()
                            .fg(Color::Rgb(215, 220, 235))
                            .add_modifier(Modifier::BOLD),
                    )
                    .title(" action "),
            ),
            lines_before_composer,
            total_lines,
        )
    }

    #[cfg(test)]
    pub(crate) fn render_lines_for_test(
        &self,
        mode: FrontendMode,
        status_text: &str,
        status_meta: &str,
        area_width: u16,
    ) -> (Vec<Line<'static>>, u16) {
        if self
            .navigator
            .active_view()
            .is_some_and(|view| view.requires_action())
        {
            let (widget, lines_before, _) =
                self.render_request_view(mode, None, status_text, None, status_meta, area_width);
            let text = format!("{widget:?}");
            return (vec![Line::raw(text)], lines_before);
        }

        let inner_width = area_width.saturating_sub(2) as usize;
        let snapshot = self.build_snapshot(
            Rect::new(0, 0, area_width, self.desired_height(mode, area_width)),
            mode,
            None,
            status_text,
            None,
            status_meta,
            "",
            inner_width,
        );
        let cursor_y = snapshot.cursor_position.map(|(_, y)| y).unwrap_or_default();
        (snapshot.input_lines, cursor_y)
    }

    pub fn desired_height(&self, mode: FrontendMode, area_width: u16) -> u16 {
        let inner_width = area_width.saturating_sub(2) as usize;
        if let Some(view) = self.navigator.active_view()
            && view.requires_action()
        {
            return (4 + view.desired_height(area_width.saturating_sub(2))).max(7);
        }

        let snapshot = self.build_snapshot(
            Rect::new(0, 0, area_width, u16::MAX),
            mode,
            None,
            "",
            None,
            "",
            "",
            inner_width,
        );
        snapshot.height
    }

    pub fn cursor_position(
        &self,
        area: Rect,
        lines_before: u16,
        mode: FrontendMode,
    ) -> Option<(u16, u16)> {
        let inner = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1).saturating_add(lines_before),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(lines_before + 2),
        };
        if let Some(view) = self.navigator.active_view() {
            let view_area = Rect {
                x: area.x.saturating_add(1),
                y: area.y.saturating_add(3),
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(4),
            };
            return view.cursor_position(view_area);
        }
        Some(self.composer.cursor_position(inner, mode))
    }

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

    pub(crate) fn set_server_request(&mut self, request: ServerRequestInlineState) {
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
        self.navigator.retain(|view| !view.is_session_picker());
    }

    pub fn set_session_picker_loading(&mut self, generation: u64, mode: SessionPickerMode) {
        self.navigator
            .replace(Box::new(SessionPickerLoading::new(generation, mode)));
    }

    pub fn is_session_picker_loading(&self, generation: u64) -> bool {
        self.navigator
            .active_view()
            .is_some_and(|view| view.is_session_picker_loading(generation))
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

    pub fn is_model_picker_loading(&self) -> bool {
        self.navigator
            .active_view()
            .is_some_and(|view| view.is_model_picker_loading())
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

    pub fn requires_action(&self) -> bool {
        self.navigator
            .active_view()
            .is_some_and(|view| view.requires_action())
    }

    pub fn composer_is_empty(&self) -> bool {
        self.navigator.is_empty() && self.composer.is_empty()
    }

    pub(crate) fn has_modal_or_popup_active(&self) -> bool {
        self.navigator.has_active_view() || self.composer.has_completion_menu()
    }

    pub(crate) fn no_modal_or_popup_active(&self) -> bool {
        !self.has_modal_or_popup_active()
    }

    #[allow(clippy::too_many_arguments)]
    fn build_snapshot(
        &self,
        area: Rect,
        mode: FrontendMode,
        status_indicator: Option<&str>,
        status_text: &str,
        runtime_hint: Option<&str>,
        status_meta: &str,
        hint_meta: &str,
        inner_width: usize,
    ) -> InputPaneSnapshot {
        let composer = self.composer.render(mode, inner_width);
        let completion_lines = if let Some(view) = self.navigator.active_view() {
            view.render_lines(area.width.saturating_sub(2))
        } else {
            composer.completion_lines.clone()
        };
        let layout = compute_input_layout(area, composer.height, completion_lines.len());
        let mut input_lines = vec![status_line(
            mode,
            status_indicator,
            status_text,
            runtime_hint,
            status_meta,
            inner_width,
        )];
        if COMPOSER_TOP_SPACER_HEIGHT > 0 {
            input_lines.push(Line::raw(""));
        }
        input_lines.extend(composer.lines);
        if layout.completion_area.is_none() {
            input_lines.push(hint_line(mode, inner_width, hint_meta));
        }
        let cursor_position = Some(self.composer.cursor_position(layout.composer_area, mode));
        let height = compute_desired_height(composer.height, completion_lines.len());

        InputPaneSnapshot {
            layout,
            input_lines,
            completion_lines,
            cursor_position,
            height,
        }
    }
}

fn compute_input_layout(
    area: Rect,
    composer_height: u16,
    completion_line_count: usize,
) -> InputPaneLayout {
    let input_content_height = STATUS_ROW_HEIGHT
        .saturating_add(COMPOSER_TOP_SPACER_HEIGHT)
        .saturating_add(composer_height)
        .saturating_add(if completion_line_count == 0 {
            COMPOSER_BOTTOM_SPACER_HEIGHT.saturating_add(HINT_ROW_HEIGHT)
        } else {
            0
        });
    let input_height = input_content_height.saturating_add(INPUT_BLOCK_CHROME_HEIGHT);
    let (input_area, completion_area) = if completion_line_count == 0 {
        (area, None)
    } else {
        let requested = (completion_line_count as u16).saturating_add(1);
        let input_height = input_height.min(area.height);
        let completion_height = requested.min(area.height.saturating_sub(input_height));
        let input_area = Rect {
            height: input_height,
            ..area
        };
        let completion_area = (completion_height > 0).then_some(Rect {
            x: area.x,
            y: area.y.saturating_add(input_height),
            width: area.width,
            height: completion_height,
        });
        (input_area, completion_area)
    };

    let composer_area = Rect {
        x: input_area.x.saturating_add(1),
        y: input_area
            .y
            .saturating_add(1 + STATUS_ROW_HEIGHT + COMPOSER_TOP_SPACER_HEIGHT),
        width: input_area.width.saturating_sub(2),
        height: composer_height.min(input_area.height.saturating_sub(
            INPUT_BLOCK_CHROME_HEIGHT + STATUS_ROW_HEIGHT + COMPOSER_TOP_SPACER_HEIGHT,
        )),
    };

    InputPaneLayout {
        input_area,
        composer_area,
        completion_area,
    }
}

fn compute_desired_height(composer_height: u16, completion_line_count: usize) -> u16 {
    let input_content_height = STATUS_ROW_HEIGHT
        .saturating_add(COMPOSER_TOP_SPACER_HEIGHT)
        .saturating_add(composer_height)
        .saturating_add(if completion_line_count == 0 {
            COMPOSER_BOTTOM_SPACER_HEIGHT.saturating_add(HINT_ROW_HEIGHT)
        } else {
            0
        });
    let input_height = input_content_height.saturating_add(INPUT_BLOCK_CHROME_HEIGHT);
    if completion_line_count == 0 {
        input_height.max(
            STATUS_ROW_HEIGHT
                .saturating_add(COMPOSER_TOP_SPACER_HEIGHT)
                .saturating_add(1)
                .saturating_add(COMPOSER_BOTTOM_SPACER_HEIGHT)
                .saturating_add(HINT_ROW_HEIGHT)
                .saturating_add(INPUT_BLOCK_CHROME_HEIGHT),
        )
    } else {
        input_height.saturating_add(completion_line_count as u16 + 1)
    }
}

impl Default for InputPane {
    fn default() -> Self {
        Self::new()
    }
}

fn border_style(mode: FrontendMode) -> Style {
    match mode {
        FrontendMode::Idle => Style::default().fg(Color::Rgb(75, 82, 110)),
        FrontendMode::Running => Style::default().fg(Color::Rgb(82, 130, 190)),
        FrontendMode::WaitingForServerRequest => Style::default().fg(Color::Rgb(210, 150, 45)),
    }
}

fn input_block(lines: Vec<Line<'static>>, border_style: Style) -> Paragraph<'static> {
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style)
            .title_style(
                Style::default()
                    .fg(Color::Rgb(215, 220, 235))
                    .add_modifier(Modifier::BOLD),
            )
            .title(" prompt "),
    )
}

#[cfg(test)]
mod tests;
