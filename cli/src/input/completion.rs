use crate::input::slash_command::{SlashCommand, SlashCommandSpec};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillCompletion {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompletionSelection {
    Command(SlashCommand),
    FilterValue(&'static str),
    Skill(SkillCompletion),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletionSuggestion {
    pub(crate) selection: CompletionSelection,
    pub(crate) label: String,
    pub(crate) description: String,
    pub(crate) insertion: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CompletionContext {
    Slash,
    Filter,
    SkillMention { replace_range: Range<usize> },
}

#[derive(Debug, Default, Clone)]
pub(crate) struct CompletionState {
    suggestions: Vec<CompletionSuggestion>,
    selected: usize,
    active: bool,
    prefix: String,
    context: Option<CompletionContext>,
}

impl CompletionState {
    pub(crate) fn sync_from_input(
        &mut self,
        text: &str,
        byte_cursor: usize,
        skills: &[SkillCompletion],
    ) {
        if let Some(arg_prefix) = filter_arg_prefix_at_cursor(text, byte_cursor) {
            if self.prefix != format!("filter:{arg_prefix}") {
                self.selected = 0;
                self.prefix = format!("filter:{arg_prefix}");
            }
            self.suggestions = filter_value_suggestions(arg_prefix);
            self.active = true;
            self.context = Some(CompletionContext::Filter);
            if self.selected >= self.suggestions.len() {
                self.selected = 0;
            }
            return;
        }

        if let Some((prefix, replace_range)) = skill_prefix_at_cursor(text, byte_cursor) {
            if self.prefix != format!("skill:{prefix}") {
                self.selected = 0;
                self.prefix = format!("skill:{prefix}");
            }
            self.suggestions = skill_suggestions(prefix, skills);
            self.active = true;
            self.context = Some(CompletionContext::SkillMention { replace_range });
            if self.selected >= self.suggestions.len() {
                self.selected = 0;
            }
            return;
        }

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
            .map(CompletionSuggestion::from)
            .collect();
        self.active = true;
        self.context = Some(CompletionContext::Slash);
        if self.selected >= self.suggestions.len() {
            self.selected = 0;
        }
    }

    pub(crate) fn clear(&mut self) {
        self.suggestions.clear();
        self.selected = 0;
        self.active = false;
        self.prefix.clear();
        self.context = None;
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active && !self.suggestions.is_empty()
    }

    pub(crate) fn suggestions(&self) -> &[CompletionSuggestion] {
        &self.suggestions
    }

    pub(crate) fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn visible_window(&self, max_rows: usize) -> (usize, &[CompletionSuggestion]) {
        if max_rows == 0 || self.suggestions.is_empty() {
            return (0, &[]);
        }
        let len = self.suggestions.len();
        let visible_len = len.min(max_rows);
        let start = if self.selected < visible_len {
            0
        } else {
            (self.selected + 1).saturating_sub(visible_len)
        }
        .min(len.saturating_sub(visible_len));
        let end = start + visible_len;
        (start, &self.suggestions[start..end])
    }

    pub(crate) fn selected(&self) -> Option<&CompletionSuggestion> {
        self.suggestions.get(self.selected)
    }

    pub(crate) fn selected_skill_replace_range(&self) -> Option<Range<usize>> {
        match &self.context {
            Some(CompletionContext::SkillMention { replace_range }) => Some(replace_range.clone()),
            _ => None,
        }
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

impl From<SlashCommandSpec> for CompletionSuggestion {
    fn from(spec: SlashCommandSpec) -> Self {
        Self {
            selection: CompletionSelection::Command(spec.command),
            label: format!("/{}", spec.name),
            description: spec.description.to_string(),
            insertion: spec.name.to_string(),
        }
    }
}

fn filter_value_suggestions(prefix: &str) -> Vec<CompletionSuggestion> {
    let values: [(&str, &str); 2] = [
        ("on", "enable pre-LLM input filtering"),
        ("off", "disable pre-LLM input filtering"),
    ];
    values
        .into_iter()
        .filter(|(value, _)| prefix.is_empty() || value.starts_with(prefix))
        .map(|(value, description)| CompletionSuggestion {
            selection: CompletionSelection::FilterValue(value),
            label: value.to_string(),
            description: description.to_string(),
            insertion: value.to_string(),
        })
        .collect()
}

fn skill_suggestions(prefix: &str, skills: &[SkillCompletion]) -> Vec<CompletionSuggestion> {
    let prefix = prefix.to_ascii_lowercase();
    skills
        .iter()
        .filter(|skill| {
            prefix.is_empty() || skill.name.to_ascii_lowercase().starts_with(prefix.as_str())
        })
        .map(|skill| CompletionSuggestion {
            selection: CompletionSelection::Skill(skill.clone()),
            label: format!("${}", skill.name),
            description: skill.description.clone(),
            insertion: format!("${}", skill.name),
        })
        .collect()
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

fn skill_prefix_at_cursor(text: &str, byte_cursor: usize) -> Option<(&str, Range<usize>)> {
    if !text.is_char_boundary(byte_cursor) {
        return None;
    }

    let char_cursor = text[..byte_cursor].chars().count();
    let mut name_start = char_cursor;
    while name_start > 0 {
        let ch = text.chars().nth(name_start - 1)?;
        if is_skill_name_char(ch) {
            name_start -= 1;
            continue;
        }
        break;
    }

    if name_start == 0 {
        return None;
    }
    let sigil_index = name_start - 1;
    if text.chars().nth(sigil_index)? != '$' {
        return None;
    }
    if sigil_index > 0 {
        let previous = text.chars().nth(sigil_index - 1)?;
        if !previous.is_whitespace() && previous != '(' && previous != '[' {
            return None;
        }
    }

    let mut end = name_start;
    while end < text.chars().count() {
        let ch = text.chars().nth(end)?;
        if is_skill_name_char(ch) {
            end += 1;
            continue;
        }
        break;
    }
    if char_cursor < name_start || char_cursor > end {
        return None;
    }

    let prefix_start = byte_index_from_char_index(text, name_start);
    let prefix_end = byte_index_from_char_index(text, byte_cursor);
    let prefix = &text[prefix_start..prefix_end];
    Some((prefix, sigil_index..end))
}

fn is_skill_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':')
}

fn byte_index_from_char_index(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn filter_arg_prefix_at_cursor(text: &str, byte_cursor: usize) -> Option<&str> {
    if !text.is_char_boundary(byte_cursor) || !text.starts_with("/filter") {
        return None;
    }
    let first_line_end = text.find('\n').unwrap_or(text.len());
    if byte_cursor > first_line_end {
        return None;
    }
    let line = &text[..first_line_end];
    let after_cmd = line.strip_prefix("/filter")?;
    if !after_cmd.is_empty() && !after_cmd.starts_with(char::is_whitespace) {
        return None;
    }
    let arg_start = "/filter".len();
    if byte_cursor < arg_start {
        return None;
    }
    let cursor_slice = &text[arg_start..byte_cursor];
    if cursor_slice.contains('\n') {
        return None;
    }
    let prefix = cursor_slice.trim_start();
    if prefix.contains(char::is_whitespace) {
        return None;
    }
    Some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_skills() -> Vec<SkillCompletion> {
        vec![
            SkillCompletion {
                name: "repo-reader".to_string(),
                description: "Read repository structure".to_string(),
                path: "D:\\repo\\.cloudagent\\skills\\repo-reader\\SKILL.md".to_string(),
            },
            SkillCompletion {
                name: "release-helper".to_string(),
                description: "Prepare releases".to_string(),
                path: "D:\\repo\\.cloudagent\\skills\\release-helper\\SKILL.md".to_string(),
            },
        ]
    }

    #[test]
    fn slash_opens_full_command_list() {
        let mut state = CompletionState::default();
        state.sync_from_input("/", 1, &[]);
        assert!(state.is_active());
        assert!(state.suggestions().len() >= 4);
    }

    #[test]
    fn filters_by_prefix() {
        let mut state = CompletionState::default();
        state.sync_from_input("/co", 3, &[]);
        let names = state
            .suggestions()
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["/copy", "/compact", "/config"]);
    }

    #[test]
    fn reasoning_command_is_in_completion_results() {
        let mut state = CompletionState::default();
        state.sync_from_input("/re", 3, &[]);
        let names = state
            .suggestions()
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"/reasoning"));
    }

    #[test]
    fn detects_skill_mentions_at_cursor() {
        let mut state = CompletionState::default();
        state.sync_from_input("please use $rep", "please use $rep".len(), &sample_skills());
        let labels = state
            .suggestions()
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["$repo-reader"]);
        assert_eq!(state.selected_skill_replace_range(), Some(11..15));
    }

    #[test]
    fn ignores_plain_dollar_without_name_context() {
        let mut state = CompletionState::default();
        state.sync_from_input("price is $5", "price is $5".len(), &sample_skills());
        assert!(!state.is_active());
    }
}
