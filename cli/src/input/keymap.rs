use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(crate) fn matches_ctrl_char(key: KeyEvent, ch: char) -> bool {
    matches!(key.code, KeyCode::Char(code) if code.eq_ignore_ascii_case(&ch))
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

pub(crate) fn matches_image_paste_shortcut(key: KeyEvent) -> bool {
    (matches!(key.code, KeyCode::Char(code) if code.eq_ignore_ascii_case(&'v') || code == '\u{16}')
        && key.modifiers.contains(KeyModifiers::CONTROL))
        || matches!(key.code, KeyCode::Char('\u{16}'))
        || (matches!(key.code, KeyCode::Insert) && key.modifiers == KeyModifiers::SHIFT)
}

#[cfg(test)]
mod tests {
    use super::matches_image_paste_shortcut;
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
}
