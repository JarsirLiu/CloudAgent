mod result;

pub(crate) use result::NavigationKeyResult;

use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction, ViewCompletion};
use agent_protocol::RequestId;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(crate) struct BottomPaneNavigator {
    stack: Vec<Box<dyn BottomPaneView>>,
}

impl BottomPaneNavigator {
    pub(crate) fn new() -> Self {
        Self { stack: Vec::new() }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    pub(crate) fn has_active_view(&self) -> bool {
        !self.stack.is_empty()
    }

    pub(crate) fn active_view(&self) -> Option<&dyn BottomPaneView> {
        self.stack.last().map(std::convert::AsRef::as_ref)
    }

    pub(crate) fn active_view_mut(&mut self) -> Option<&mut (dyn BottomPaneView + '_)> {
        if let Some(view) = self.stack.last_mut() {
            Some(view.as_mut())
        } else {
            None
        }
    }

    pub(crate) fn push(&mut self, view: Box<dyn BottomPaneView>) {
        self.stack.push(view);
    }

    pub(crate) fn replace(&mut self, view: Box<dyn BottomPaneView>) {
        self.clear();
        self.push(view);
    }

    pub(crate) fn replace_active(&mut self, view: Box<dyn BottomPaneView>) {
        if let Some(active) = self.stack.last_mut() {
            *active = view;
        } else {
            self.push(view);
        }
    }

    pub(crate) fn replace_parent_after_child(&mut self, view: Box<dyn BottomPaneView>) {
        if !self.stack.is_empty() {
            self.stack.pop();
        }
        self.replace_active(view);
    }

    pub(crate) fn clear(&mut self) {
        self.stack.clear();
    }

    pub(crate) fn retain<F>(&mut self, keep: F)
    where
        F: FnMut(&Box<dyn BottomPaneView>) -> bool,
    {
        self.stack.retain(keep);
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> NavigationKeyResult {
        let Some(view) = self.active_view_mut() else {
            return NavigationKeyResult::NoActiveView;
        };

        let view_requires_action = view.requires_action();
        if key.code == KeyCode::Esc
            && key.modifiers == KeyModifiers::NONE
            && !view.prefer_esc_to_handle_key_event()
        {
            if view_requires_action {
                return NavigationKeyResult::FallthroughEscFromActionRequiredView;
            }
            self.pop_with_completion(Some(ViewCompletion::Cancelled));
            return NavigationKeyResult::Consumed;
        }

        let action = view.handle_key_event(key);
        match action {
            BottomPaneViewAction::None => {
                let complete = view.is_complete();
                let completion = view.completion();
                if complete {
                    self.pop_with_completion(completion);
                    return NavigationKeyResult::Consumed;
                }
                if key.code == KeyCode::Esc
                    && key.modifiers == KeyModifiers::NONE
                    && view_requires_action
                {
                    return NavigationKeyResult::FallthroughEscFromActionRequiredView;
                }
                NavigationKeyResult::Consumed
            }
            BottomPaneViewAction::Cancel | BottomPaneViewAction::Back => {
                self.pop_with_completion(Some(ViewCompletion::Cancelled));
                NavigationKeyResult::Consumed
            }
            BottomPaneViewAction::Composer(intent) => {
                if !matches!(intent, crate::input::intent::ComposerIntent::None) {
                    self.pop_with_completion(Some(ViewCompletion::Accepted));
                    NavigationKeyResult::Composer(intent)
                } else {
                    NavigationKeyResult::Consumed
                }
            }
            BottomPaneViewAction::ComposerWithoutDismiss(intent) => {
                if !matches!(intent, crate::input::intent::ComposerIntent::None) {
                    NavigationKeyResult::Composer(intent)
                } else {
                    NavigationKeyResult::Consumed
                }
            }
            BottomPaneViewAction::ServerRequestSubmit {
                request_id,
                decision,
                reason,
            } => {
                let complete = self.active_view().is_some_and(BottomPaneView::is_complete);
                let completion = self.active_view().and_then(BottomPaneView::completion);
                if complete {
                    self.pop_with_completion(completion);
                }
                NavigationKeyResult::ServerRequestSubmit {
                    request_id,
                    decision,
                    reason,
                }
            }
        }
    }

    pub(crate) fn handle_paste(&mut self, text: &str) -> Option<BottomPaneViewAction> {
        self.active_view_mut().map(|view| view.handle_paste(text))
    }

    pub(crate) fn dismiss_server_request(&mut self, request_id: &RequestId) {
        let Some(view) = self.active_view_mut() else {
            return;
        };
        if !view.dismiss_server_request(request_id) {
            return;
        }
        let complete = view.is_complete();
        let completion = view.completion();
        if complete {
            self.pop_with_completion(completion);
        }
    }

    pub(crate) fn pop_with_completion(&mut self, completion: Option<ViewCompletion>) {
        if self.stack.pop().is_none() {
            return;
        }
        match completion {
            Some(ViewCompletion::Accepted) => {
                while self
                    .stack
                    .last()
                    .is_some_and(|view| view.dismiss_after_child_accept())
                {
                    self.stack.pop();
                }
            }
            Some(ViewCompletion::Cancelled) => {
                if let Some(view) = self.stack.last_mut() {
                    view.clear_dismiss_after_child_accept();
                }
            }
            None => {}
        }
    }
}

impl Default for BottomPaneNavigator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
