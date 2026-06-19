use super::{InputPane, InputPaneAction};
use crate::input::intent::ComposerIntent;
use crate::ui::bottom_pane_navigation::NavigationKeyResult;
use crate::ui::widgets::bottom_pane_view::BottomPaneViewAction;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl InputPane {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<InputPaneAction> {
        match self.navigator.handle_key(key) {
            NavigationKeyResult::NoActiveView => {}
            NavigationKeyResult::Handled => {
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
        }

        if matches!(key.kind, crossterm::event::KeyEventKind::Press)
            && key.code == KeyCode::Esc
            && key.modifiers.is_empty()
        {
            return self.handle_escape_key();
        }

        self.composer.handle_key(key).map(InputPaneAction::Composer)
    }

    fn handle_escape_key(&mut self) -> Option<InputPaneAction> {
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
}
