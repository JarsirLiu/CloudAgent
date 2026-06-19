use super::BottomPaneNavigator;
use crate::ui::bottom_pane::bottom_pane_view::{BottomPaneView, ViewCompletion};

impl BottomPaneNavigator {
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

