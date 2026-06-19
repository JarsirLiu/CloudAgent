use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{TextArea, TextAreaState, wrap_text};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn wrap_text_preserves_trailing_space_visibility() {
    assert_eq!(wrap_text("abc ", 10), vec!["abc "]);
    assert_eq!(wrap_text("abc ", 3), vec!["abc", " "]);
}

#[test]
fn wrap_text_preserves_consecutive_spaces() {
    assert_eq!(wrap_text("a  b", 10), vec!["a  b"]);
    assert_eq!(wrap_text("a  b", 2), vec!["a ", " b"]);
}

#[test]
fn ctrl_a_selects_all() {
    let mut ta = TextArea::new();
    ta.set_text("hello\nworld");
    ta.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    assert_eq!(ta.selected_text().as_deref(), Some("hello\nworld"));
}

#[test]
fn down_moves_to_next_line_or_text_end() {
    let mut ta = TextArea::new();
    ta.set_text("first\nsecond\nthird");
    ta.handle_key(key(KeyCode::Home));
    ta.handle_key(key(KeyCode::Down));

    assert_eq!(
        ta.text().chars().take(ta.cursor()).collect::<String>(),
        "first\n"
    );

    ta.handle_key(key(KeyCode::Down));
    ta.handle_key(key(KeyCode::Down));
    assert_eq!(ta.cursor(), ta.text().chars().count());
}

#[test]
fn ctrl_x_cuts_selected_text() {
    let mut ta = TextArea::new();
    ta.set_text("hello");
    ta.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
    ta.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
    ta.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));

    assert_eq!(ta.text(), "hel");
    assert_eq!(ta.cursor(), 3);
}

#[test]
fn ctrl_a_then_ctrl_x_cuts_entire_buffer() {
    let mut ta = TextArea::new();
    ta.set_text("hello\nworld");
    ta.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    ta.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));

    assert!(ta.text().is_empty());
    assert_eq!(ta.cursor(), 0);
}

#[test]
fn ctrl_z_restores_previous_text() {
    let mut ta = TextArea::new();
    ta.insert_str("alpha");
    ta.insert_str("beta");
    ta.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));

    assert_eq!(ta.text(), "alpha");
}

#[test]
fn shift_left_creates_selection() {
    let mut ta = TextArea::new();
    ta.set_text("hello");
    ta.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));

    assert_eq!(ta.selected_text().as_deref(), Some("o"));
}

#[test]
fn ctrl_u_k_and_y_follow_shell_style_editing() {
    let mut ta = TextArea::new();
    ta.set_text("hello world");
    ta.handle_key(key(KeyCode::End));
    ta.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert_eq!(ta.text(), "");

    ta.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert_eq!(ta.text(), "hello world");

    ta.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    ta.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    assert_eq!(ta.text(), "");

    ta.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert_eq!(ta.text(), "hello world");
}

#[test]
fn up_down_follow_visual_wrapped_lines_after_render() {
    let mut ta = TextArea::new();
    ta.set_text("abcdef");
    let mut state = TextAreaState::default();
    let _ = ta.visible_wrapped_lines(ta.text(), 3, 2, &mut state);

    ta.handle_key(key(KeyCode::End));
    ta.handle_key(key(KeyCode::Up));
    assert_eq!(ta.cursor(), 3);

    ta.handle_key(key(KeyCode::Down));
    assert_eq!(ta.cursor(), 6);
}

#[test]
fn ctrl_p_n_and_alt_word_bindings_match_shell_style_defaults() {
    let mut ta = TextArea::new();
    ta.set_text("alpha beta");

    ta.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    ta.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));
    assert_eq!(ta.cursor(), 5);

    ta.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    assert_eq!(ta.cursor(), ta.text().chars().count());

    ta.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert_eq!(ta.cursor(), 0);

    ta.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(ta.cursor(), 5);

    ta.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
    assert_eq!(ta.cursor(), 0);
}

#[test]
fn delete_and_alt_d_delete_forward() {
    let mut ta = TextArea::new();
    ta.set_text("alpha beta");
    ta.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    ta.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    assert_eq!(ta.text(), "lpha beta");

    ta.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT));
    assert_eq!(ta.text(), " beta");
}
