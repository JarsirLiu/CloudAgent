use crate::input::intent::ComposerIntent;
use crate::ui::bottom_pane::chat_composer::ChatComposer;
pub(crate) use crate::ui::bottom_pane::dialogs::server_request::server_request_model::ServerRequestInlineState;
use crate::ui::bottom_pane_navigation::BottomPaneNavigator;
use agent_core::ServerRequestDecisionKind;
use agent_core::SkillMetadata;
use agent_protocol::RequestId;
use std::path::PathBuf;
use std::time::Duration;

mod key_routing;
mod layout;
mod render;
mod view_factory;

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

impl InputPane {
    pub fn new() -> Self {
        Self {
            composer: ChatComposer::new(),
            navigator: BottomPaneNavigator::new(),
        }
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

    pub(crate) fn supports_text_paste_shortcut(&self) -> bool {
        self.navigator
            .active_view()
            .is_some_and(|view| view.supports_text_paste_shortcut())
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
}

impl Default for InputPane {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "esc_tests.rs"]
mod tests;
