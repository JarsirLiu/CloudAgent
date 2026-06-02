use crate::ui::widgets::form_field_editor::FormFieldEditor;

#[derive(Default)]
pub(crate) struct FormInputState {
    editor: FormFieldEditor,
}

impl FormInputState {
    pub(crate) fn new() -> Self {
        Self {
            editor: FormFieldEditor::new_with_replace_on_next_input(),
        }
    }

    pub(crate) fn move_selection(&mut self, selected: &mut usize, next: usize) {
        let clamped = next;
        if clamped != *selected {
            *selected = clamped;
            self.editor.mark_replace_on_next_input();
        }
    }

    pub(crate) fn append_paste(&mut self, field: &mut String, text: &str) {
        self.editor.append_paste(field, text);
    }

    pub(crate) fn append_char(&mut self, field: &mut String, ch: char) -> bool {
        self.editor.append_char(field, ch)
    }

    pub(crate) fn backspace(&mut self, field: &mut String) {
        self.editor.backspace(field);
    }

    pub(crate) fn clear_paste_echo(&mut self) {
        self.editor.clear_paste_echo();
    }
}

#[cfg(test)]
mod tests {
    use super::FormInputState;

    #[test]
    fn selection_change_resets_replace_mode() {
        let mut state = FormInputState::new();
        let mut selected = 0usize;
        state.move_selection(&mut selected, 1);
        assert_eq!(selected, 1);
    }
}
