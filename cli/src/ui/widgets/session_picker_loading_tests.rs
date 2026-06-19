use super::SessionPickerLoading;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction, ViewKind};
use crate::ui::widgets::session_picker::SessionPickerMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

#[test]
fn render_lines_describe_loading_state() {
    let view = SessionPickerLoading::new(SessionPickerMode::Switch);
    let lines = view
        .render_lines(80)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    assert!(lines.iter().any(|line| line.contains("Session Picker")));
    assert!(lines.iter().any(|line| line.contains("Loading sessions")));
    assert!(lines.iter().any(|line| line.contains("Esc to cancel")));
}

#[test]
fn esc_cancels_loading_view() {
    let mut view = SessionPickerLoading::new(SessionPickerMode::Switch);

    assert!(matches!(
        view.handle_key_event(press(KeyCode::Esc)),
        BottomPaneViewAction::Cancel
    ));
}

#[test]
fn ignores_key_release() {
    let mut view = SessionPickerLoading::new(SessionPickerMode::Switch);

    assert!(matches!(
        view.handle_key_event(KeyEvent {
            code: KeyCode::Esc,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        }),
        BottomPaneViewAction::None
    ));
}

#[test]
fn loading_view_reports_kind_and_generation() {
    let view = SessionPickerLoading::new(SessionPickerMode::Delete);

    assert_eq!(view.kind(), ViewKind::SessionPickerLoading);
}
