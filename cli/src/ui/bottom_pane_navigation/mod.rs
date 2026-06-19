mod route;
mod result;
mod stack;

pub(crate) use result::NavigationKeyResult;

use crate::ui::bottom_pane::bottom_pane_view::BottomPaneView;

pub(crate) struct BottomPaneNavigator {
    stack: Vec<Box<dyn BottomPaneView>>,
}

impl BottomPaneNavigator {
    pub(crate) fn new() -> Self {
        Self { stack: Vec::new() }
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

