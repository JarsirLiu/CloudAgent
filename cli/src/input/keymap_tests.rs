use super::{matches_image_paste_shortcut, matches_insert_newline_shortcut};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn accepts_ctrl_v() {
    assert!(matches_image_paste_shortcut(KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL
    )));
}

#[test]
fn accepts_ctrl_alt_v() {
    assert!(matches_image_paste_shortcut(KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL | KeyModifiers::ALT
    )));
}

#[test]
fn accepts_shift_insert() {
    assert!(matches_image_paste_shortcut(KeyEvent::new(
        KeyCode::Insert,
        KeyModifiers::SHIFT
    )));
}

#[test]
fn accepts_ctrl_v_control_char() {
    assert!(matches_image_paste_shortcut(KeyEvent::new(
        KeyCode::Char('\u{16}'),
        KeyModifiers::CONTROL
    )));
}

#[test]
fn accepts_bare_ctrl_v_control_char() {
    assert!(matches_image_paste_shortcut(KeyEvent::new(
        KeyCode::Char('\u{16}'),
        KeyModifiers::NONE
    )));
}

#[test]
fn accepts_shift_enter() {
    assert!(matches_insert_newline_shortcut(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::SHIFT
    )));
}

#[test]
fn accepts_ctrl_enter() {
    assert!(matches_insert_newline_shortcut(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL
    )));
}

#[test]
fn accepts_alt_enter() {
    assert!(matches_insert_newline_shortcut(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::ALT
    )));
}
