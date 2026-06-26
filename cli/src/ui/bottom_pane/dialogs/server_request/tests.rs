use super::*;
use crossterm::event::KeyModifiers;

fn request_state(id: &str) -> ServerRequestInlineState {
    ServerRequestInlineState {
        request_id: RequestId::String(id.to_string()),
        presentation:
            crate::ui::bottom_pane::dialogs::server_request::server_request_model::ServerRequestPresentation::command(
                "exec_command",
                "needs review",
                "Get-Content file.rs",
            ),
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn type_text(overlay: &mut ServerRequestOverlay, text: &str) {
    for ch in text.chars() {
        overlay.handle_key_event(key(KeyCode::Char(ch)));
    }
}

#[test]
fn cursor_is_hidden_until_a_note_is_typed() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));

    assert!(overlay.cursor_position(Rect::new(0, 20, 80, 16)).is_none());

    type_text(&mut overlay, "because");

    let (_x, y) = overlay
        .cursor_position(Rect::new(0, 20, 80, 16))
        .expect("cursor");

    assert_eq!(y, 28);
}

#[test]
fn numeric_shortcuts_submit_matching_decisions() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));

    let action = overlay.handle_key_event(key(KeyCode::Char('3')));

    assert!(matches!(
        action,
        BottomPaneViewAction::ServerRequestSubmit {
            decision: ServerRequestDecisionKind::Decline,
            ..
        }
    ));
}

#[test]
fn enter_uses_the_selected_decision_even_with_reason_text() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));
    overlay.selected = 2;
    type_text(&mut overlay, "because");

    let action = overlay.handle_key_event(key(KeyCode::Enter));

    assert!(matches!(
        action,
        BottomPaneViewAction::ServerRequestSubmit {
            decision: ServerRequestDecisionKind::Decline,
            reason,
            ..
        } if reason == "because"
    ));
}

#[test]
fn slash_command_in_request_overlay_dispatches_global_intent() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));
    type_text(&mut overlay, "/interrupt");

    let action = overlay.handle_key_event(key(KeyCode::Enter));

    assert!(matches!(
        action,
        BottomPaneViewAction::Composer(ComposerIntent::Interrupt)
    ));
}

#[test]
fn slash_unknown_in_request_overlay_is_not_treated_as_approval_reason() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));
    type_text(&mut overlay, "/wat");

    let action = overlay.handle_key_event(key(KeyCode::Enter));

    assert!(matches!(
        action,
        BottomPaneViewAction::Composer(ComposerIntent::UnknownCommand(command))
            if command == "wat"
    ));
}

#[test]
fn long_note_wraps_and_expands_height() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));
    type_text(
        &mut overlay,
        "please skip this because the command has not been reviewed yet",
    );

    let height = overlay.desired_height(32);
    let lines = overlay.render_lines(32);

    assert!(height > COMPACT_APPROVAL_HEIGHT);
    assert_eq!(height as usize, lines.len());
    assert!(lines.iter().any(|line| {
        line.spans
            .iter()
            .any(|span| span.content.contains("reviewed"))
    }));
}

#[cfg(windows)]
#[test]
fn altgr_character_is_inserted_in_note() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));

    overlay.handle_key_event(KeyEvent {
        code: KeyCode::Char('@'),
        modifiers: KeyModifiers::CONTROL | KeyModifiers::ALT,
        kind: KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    });

    assert_eq!(overlay.reply.text(), "@");
}

#[test]
fn shift_enter_inserts_newline_in_note() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));
    type_text(&mut overlay, "first");

    let action = overlay.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    type_text(&mut overlay, "second");

    assert!(matches!(action, BottomPaneViewAction::None));
    assert_eq!(overlay.reply.text(), "first\nsecond");
}

#[test]
fn ctrl_enter_inserts_newline_in_note() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));
    type_text(&mut overlay, "first");

    let action = overlay.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    type_text(&mut overlay, "second");

    assert!(matches!(action, BottomPaneViewAction::None));
    assert_eq!(overlay.reply.text(), "first\nsecond");
}

#[test]
fn alt_enter_inserts_newline_in_note() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));
    type_text(&mut overlay, "first");

    let action = overlay.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
    type_text(&mut overlay, "second");

    assert!(matches!(action, BottomPaneViewAction::None));
    assert_eq!(overlay.reply.text(), "first\nsecond");
}

#[test]
fn escape_is_consumed_without_dismissing_overlay() {
    let mut overlay = ServerRequestOverlay::new(request_state("req-1"));

    let action = overlay.handle_key_event(key(KeyCode::Esc));

    assert!(matches!(action, BottomPaneViewAction::Handled));
    assert!(!overlay.is_complete());
}
