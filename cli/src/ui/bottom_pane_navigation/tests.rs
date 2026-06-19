use super::*;
use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::{BottomPaneViewAction, ViewKind};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::text::Line;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn release_key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Release,
        state: crossterm::event::KeyEventState::NONE,
    }
}

struct TestView {
    action: BottomPaneViewAction,
    dismiss_after_child_accept: bool,
    cleared_child_accept: bool,
}

impl TestView {
    fn new(action: BottomPaneViewAction) -> Self {
        Self {
            action,
            dismiss_after_child_accept: false,
            cleared_child_accept: false,
        }
    }
}

impl BottomPaneView for TestView {
    fn kind(&self) -> ViewKind {
        ViewKind::Help
    }

    fn handle_key_event(&mut self, _key: KeyEvent) -> BottomPaneViewAction {
        self.action.clone()
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        Vec::new()
    }

    fn dismiss_after_child_accept(&self) -> bool {
        self.dismiss_after_child_accept
    }

    fn clear_dismiss_after_child_accept(&mut self) {
        self.cleared_child_accept = true;
        self.dismiss_after_child_accept = false;
    }
}

#[test]
fn cancel_pops_active_view() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::Cancel)));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Esc)),
        NavigationKeyResult::Handled
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
        NavigationKeyResult::Handled
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
fn esc_with_none_action_pops_active_view() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::None)));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Esc)),
        NavigationKeyResult::Handled
    ));
    assert!(navigator.is_empty());
}

#[test]
fn normal_view_esc_is_consumed() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::None)));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Esc)),
        NavigationKeyResult::Handled
    ));
    assert!(navigator.is_empty());
}

#[test]
fn esc_routes_to_view_handler_before_default_close() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::Composer(
        ComposerIntent::Help,
    ))));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Esc)),
        NavigationKeyResult::Composer(ComposerIntent::Help)
    ));
}

#[test]
fn handled_action_consumes_escape_without_dismissing_view() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::Handled)));

    assert!(matches!(
        navigator.handle_key(key(KeyCode::Esc)),
        NavigationKeyResult::Handled
    ));
    assert!(navigator.has_active_view());
}

#[test]
fn key_release_does_not_close_active_view() {
    let mut navigator = BottomPaneNavigator::new();
    navigator.push(Box::new(TestView::new(BottomPaneViewAction::None)));

    assert!(matches!(
        navigator.handle_key(release_key(KeyCode::Esc)),
        NavigationKeyResult::Handled
    ));
    assert!(navigator.has_active_view());
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
