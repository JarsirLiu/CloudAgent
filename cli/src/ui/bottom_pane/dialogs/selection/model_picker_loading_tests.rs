use super::ModelPickerLoading;
use crate::ui::bottom_pane::bottom_pane_view::{BottomPaneView, ViewKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn renders_current_model_and_loading_state() {
    let picker = ModelPickerLoading::new("gpt-test");
    let text = picker
        .render_lines(80)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("Current model: gpt-test"));
    assert!(text.contains("Loading model list"));
}

#[test]
fn enter_does_not_submit_a_model() {
    let mut picker = ModelPickerLoading::new("gpt-test");
    let action = picker.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(matches!(
        action,
        crate::ui::bottom_pane::bottom_pane_view::BottomPaneViewAction::None
    ));
}

#[test]
fn identifies_as_loading_picker() {
    let picker = ModelPickerLoading::new("gpt-test");
    assert_eq!(picker.kind(), ViewKind::ModelPickerLoading);
}
