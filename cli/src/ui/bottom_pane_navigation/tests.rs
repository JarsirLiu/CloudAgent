use super::*;
use crate::input::intent::ComposerIntent;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

struct TestView {
    action: BottomPaneViewAction,
    requires_action: bool,
    dismiss_after_child_accept: bool,
    cleared_child_accept: bool,
    prefer_esc_to_handle_key_event: bool,
}

impl TestView {
    fn new(action: BottomPaneViewAction) -> Self {
        Self {
            action,
            requires_action: false,
            dismiss_after_child_accept: false,
            cleared_child_accept: false,
            prefer_esc_to_handle_key_event: false,
        }
    }

    fn action_required() -> Self {
        Self {
            action: BottomPaneViewAction::None,
            requires_action: true,
            dismiss_after_child_accept: false,
            cleared_child_accept: false,
            prefer_esc_to_handle_key_event: false,
        }
    }
}

impl BottomPaneView for TestView {
    fn handle_key_event(&mut self, _key: KeyEvent) -> BottomPaneViewAction {
        self.action.clone()
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        Vec::new()
    }

    fn requires_action(&self) -> bool {
        self.requires_action
    }

    fn dismiss_after_child_accept(&self) -> bool {
        self.dismiss_after_child_accept
    }

    fn clear_dismiss_after_child_accept(&mut self) {
        self.cleared_child_accept = true;
        self.dismiss_after_child_accept = false;
    }

    fn prefer_esc_to_handle_key_event(&self) -> bool {
        self.prefer_esc_to_handle_key_event
    }
}

#[test]
fn cancel_pops_active_view() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::Cancel)));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Esc)),
        NavigationKeyResult::Consumed
    ));
    assert!(navigator.is_empty());
}

#[test]
fn back_pops_child_and_keeps_parent() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::None)));
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::Back)));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Esc)),
        NavigationKeyResult::Consumed
    ));
    assert!(navigator.has_active_view());
}

#[test]
fn accepted_child_can_dismiss_parent() {
    let mut parent = TestView::new(BottomPaneViewAction::None);
    parent.dismiss_after_child_accept = true;
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(parent));
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::Composer(
        ComposerIntent::Help,
    ))));

    let _ = navigator.handle_key(key(KeyCode::Enter));

    assert!(navigator.is_empty());
}

#[test]
fn cancelled_child_keeps_parent() {
    let mut parent = TestView::new(BottomPaneViewAction::None);
    parent.dismiss_after_child_accept = true;
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(parent));
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::Cancel)));

    let _ = navigator.handle_key(key(KeyCode::Esc));

    assert!(navigator.has_active_view());
}

#[test]
fn action_required_esc_can_fallthrough() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::action_required()));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Esc)),
        NavigationKeyResult::FallthroughEscFromActionRequiredView
    ));
}

#[test]
fn normal_view_esc_is_consumed() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::None)));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Esc)),
        NavigationKeyResult::Consumed
    ));
}

#[test]
fn preferred_esc_routes_to_view_handler() {
    let mut view = TestView::new(BottomPaneViewAction::Composer(ComposerIntent::Help));
    view.prefer_esc_to_handle_key_event = true;
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(view));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Esc)),
        NavigationKeyResult::Composer(ComposerIntent::Help)
    ));
}

#[test]
fn composer_action_accepts_active_view() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::Composer(
        ComposerIntent::Help,
    ))));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Enter)),
        NavigationKeyResult::Composer(ComposerIntent::Help)
    ));
    assert!(navigator.is_empty());
}

#[test]
fn composer_without_dismiss_keeps_active_view() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(
        BottomPaneViewAction::ComposerWithoutDismiss(ComposerIntent::Help),
    )));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Enter)),
        NavigationKeyResult::Composer(ComposerIntent::Help)
    ));
    assert!(navigator.has_active_view());
}
