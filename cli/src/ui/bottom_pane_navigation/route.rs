use super::{BottomPaneNavigator, NavigationKeyResult};
use crate::ui::bottom_pane::bottom_pane_view::{
    BottomPaneView, BottomPaneViewAction, ViewCompletion,
};
use agent_protocol::RequestId;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

impl BottomPaneNavigator {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> NavigationKeyResult {
        let Some(view) = self.active_view_mut() else {
            return NavigationKeyResult::NoActiveView;
        };

        if !matches!(key.kind, KeyEventKind::Press) {
            return NavigationKeyResult::Handled;
        }

        if key.code == KeyCode::Esc && key.modifiers == KeyModifiers::NONE {
            let action = view.handle_key_event(key);
            return self.handle_action_result(action, true);
        }

        let action = view.handle_key_event(key);
        self.handle_action_result(action, false)
    }

    fn handle_action_result(
        &mut self,
        action: BottomPaneViewAction,
        is_escape: bool,
    ) -> NavigationKeyResult {
        match action {
            BottomPaneViewAction::Handled => NavigationKeyResult::Handled,
            BottomPaneViewAction::None if is_escape => {
                self.pop_with_completion(Some(ViewCompletion::Cancelled));
                NavigationKeyResult::Handled
            }
            BottomPaneViewAction::None => {
                let complete = self.active_view().is_some_and(BottomPaneView::is_complete);
                let completion = self.active_view().and_then(BottomPaneView::completion);
                if complete {
                    self.pop_with_completion(completion);
                }
                NavigationKeyResult::Handled
            }
            BottomPaneViewAction::Cancel | BottomPaneViewAction::Back => {
                self.pop_with_completion(Some(ViewCompletion::Cancelled));
                NavigationKeyResult::Handled
            }
            BottomPaneViewAction::Composer(intent) => {
                if !matches!(intent, crate::input::intent::ComposerIntent::None) {
                    self.pop_with_completion(Some(ViewCompletion::Accepted));
                    NavigationKeyResult::Composer(intent)
                } else {
                    NavigationKeyResult::Handled
                }
            }
            BottomPaneViewAction::ComposerWithoutDismiss(intent) => {
                if !matches!(intent, crate::input::intent::ComposerIntent::None) {
                    NavigationKeyResult::Composer(intent)
                } else {
                    NavigationKeyResult::Handled
                }
            }
            BottomPaneViewAction::LoadMoreSessions { cursor } => {
                NavigationKeyResult::LoadMoreSessions { cursor }
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
}
