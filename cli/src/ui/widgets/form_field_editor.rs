use crate::ui::widgets::paste_echo_guard::PasteEchoGuard;

#[derive(Default)]
pub(crate) struct FormFieldEditor {
    replace_on_next_input: bool,
    paste_echo_guard: PasteEchoGuard,
}

impl FormFieldEditor {
    pub(crate) fn new_with_replace_on_next_input() -> Self {
        Self {
            replace_on_next_input: true,
            paste_echo_guard: PasteEchoGuard::default(),
        }
    }

    pub(crate) fn mark_replace_on_next_input(&mut self) {
        self.replace_on_next_input = true;
        self.paste_echo_guard.clear();
    }

    pub(crate) fn clear_paste_echo(&mut self) {
        self.paste_echo_guard.clear();
    }

    pub(crate) fn should_ignore_char(&mut self, ch: char) -> bool {
        self.paste_echo_guard.should_ignore_char(ch)
    }

    pub(crate) fn append_paste(&mut self, field: &mut String, text: &str) {
        self.prepare_field_for_input(field);
        field.push_str(text);
        self.paste_echo_guard.arm(text);
    }

    pub(crate) fn append_char(&mut self, field: &mut String, ch: char) -> bool {
        if self.should_ignore_char(ch) {
            return false;
        }
        self.prepare_field_for_input(field);
        field.push(ch);
        true
    }

    pub(crate) fn backspace(&mut self, field: &mut String) {
        self.paste_echo_guard.clear();
        self.prepare_field_for_input(field);
        field.pop();
    }

    fn prepare_field_for_input(&mut self, field: &mut String) {
        if self.replace_on_next_input {
            field.clear();
            self.replace_on_next_input = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FormFieldEditor;

    #[test]
    fn first_input_replaces_existing_value_when_armed() {
        let mut editor = FormFieldEditor::new_with_replace_on_next_input();
        let mut field = "old".to_string();
        assert!(editor.append_char(&mut field, 'n'));
        assert_eq!(field, "n");
    }

    #[test]
    fn paste_echo_is_ignored_after_explicit_paste() {
        let mut editor = FormFieldEditor::new_with_replace_on_next_input();
        let mut field = String::new();
        editor.append_paste(&mut field, "token");
        for ch in "token".chars() {
            assert!(!editor.append_char(&mut field, ch));
        }
        assert_eq!(field, "token");
    }
}
