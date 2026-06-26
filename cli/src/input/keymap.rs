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

pub(crate) fn matches_insert_newline_shortcut(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Enter)
        && key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL)
}

#[cfg(test)]
#[path = "keymap_tests.rs"]
mod tests;
