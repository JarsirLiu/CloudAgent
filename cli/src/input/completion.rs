use crate::input::slash_command::{SlashCommand, SlashCommandSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommandSuggestion {
    pub(crate) command: SlashCommand,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) argument_hint: Option<&'static str>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct CompletionState {
    suggestions: Vec<CommandSuggestion>,
    selected: usize,
    active: bool,
    prefix: String,
}

impl CompletionState {
    pub(crate) fn sync_from_input(&mut self, text: &str, byte_cursor: usize) {
        let Some(prefix) = slash_prefix_at_cursor(text, byte_cursor) else {
            self.clear();
            return;
        };

        if self.prefix != prefix {
            self.selected = 0;
            self.prefix = prefix.to_string();
        }

        self.suggestions = SlashCommand::all()
            .iter()
            .copied()
            .filter(|spec| matches_command(*spec, prefix))
            .map(CommandSuggestion::from)
            .collect();
        self.active = true;
        if self.selected >= self.suggestions.len() {
            self.selected = 0;
        }
    }

    pub(crate) fn clear(&mut self) {
        self.suggestions.clear();
        self.selected = 0;
        self.active = false;
        self.prefix.clear();
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active && !self.suggestions.is_empty()
    }

    pub(crate) fn suggestions(&self) -> &[CommandSuggestion] {
        &self.suggestions
    }

    pub(crate) fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn selected(&self) -> Option<CommandSuggestion> {
        self.suggestions.get(self.selected).copied()
    }

    pub(crate) fn move_up(&mut self) {
        if self.suggestions.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.suggestions.len() - 1
        } else {
            self.selected - 1
        };
    }

    pub(crate) fn move_down(&mut self) {
        if self.suggestions.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.suggestions.len();
    }
}

impl From<SlashCommandSpec> for CommandSuggestion {
    fn from(spec: SlashCommandSpec) -> Self {
        Self {
            command: spec.command,
            name: spec.name,
            description: spec.description,
            argument_hint: spec.argument_hint,
        }
    }
}

fn matches_command(command: SlashCommandSpec, prefix: &str) -> bool {
    prefix.is_empty() || command.matches_prefix(prefix)
}

fn slash_prefix_at_cursor(text: &str, byte_cursor: usize) -> Option<&str> {
    if !text.is_char_boundary(byte_cursor) {
        return None;
    }
    let first_line_end = text.find('\n').unwrap_or(text.len());
    if byte_cursor > first_line_end || !text.starts_with('/') {
        return None;
    }

    let token_end = text[1..first_line_end]
        .find(char::is_whitespace)
        .map(|idx| idx + 1)
        .unwrap_or(first_line_end);
    if byte_cursor > token_end {
        return None;
    }

    let prefix = &text[1..token_end];
    if prefix.contains('/') {
        return None;
    }
    Some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_opens_full_command_list() {
        let mut state = CompletionState::default();
        state.sync_from_input("/", 1);
        assert!(state.is_active());
        assert!(state.suggestions().len() >= 4);
    }

    #[test]
    fn filters_by_prefix() {
        let mut state = CompletionState::default();
        state.sync_from_input("/co", 3);
        let names = state
            .suggestions()
            .iter()
            .map(|item| item.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["copy"]);
    }

    #[test]
    fn filters_by_alias() {
        let mut state = CompletionState::default();
        state.sync_from_input("/sto", 4);
        let names = state
            .suggestions()
            .iter()
            .map(|item| item.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["interrupt"]);
    }
}
