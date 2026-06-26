use crate::input::command_dispatch::intent_for_slash_command;
use crate::input::completion::{CompletionSelection, CompletionState, SkillCompletion};
use crate::input::intent::ComposerIntent;
use crate::input::keymap::matches_insert_newline_shortcut;
use crate::input::slash_command::find_slash_command;
use crate::ui::bottom_pane::support::paste_burst::{CharDecision, FlushResult, PasteBurst};
use agent_core::conversation::{AttachmentRef, InputItem};
use agent_protocol::FrontendMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use crate::app::input::clipboard_paste::{is_supported_image_path, normalize_pasted_image_path};
use crate::ui::bottom_pane::support::textarea::{TextArea, TextAreaState, is_altgr};

mod attachments;
mod history;
mod render;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use attachments::{
    AttachedSkill, LocalAttachedImage, RemoteAttachedImage, build_submission_content,
};
use history::{ComposerHistory, HistoryNavigation};
pub use render::ComposerRender;
#[cfg(test)]
use render::MAX_VISIBLE_COMPOSER_ROWS;
use render::{composer_cursor_position, composer_desired_height, render_composer};

pub struct ChatComposer {
    textarea: TextArea,
    textarea_state: RefCell<TextAreaState>,
    completion: CompletionState,
    paste_burst: PasteBurst,
    history: ComposerHistory,
    local_images: Vec<LocalAttachedImage>,
    remote_images: Vec<RemoteAttachedImage>,
    attached_skills: Vec<AttachedSkill>,
    available_skills: Vec<SkillCompletion>,
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            textarea: TextArea::new(),
            textarea_state: RefCell::new(TextAreaState::default()),
            completion: CompletionState::default(),
            paste_burst: PasteBurst::default(),
            history: ComposerHistory::default(),
            local_images: Vec::new(),
            remote_images: Vec::new(),
            attached_skills: Vec::new(),
            available_skills: Vec::new(),
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<ComposerIntent> {
        self.flush_paste_burst_if_due();

        if !matches!(key.kind, KeyEventKind::Press) {
            return None;
        }

        if key.modifiers == (KeyModifiers::CONTROL | KeyModifiers::SHIFT)
            && key.code == KeyCode::Char('C')
            && let Some(selected) = self.textarea.selected_text()
        {
            if let Some(pasted) = self.paste_burst.flush_before_modified_input() {
                let _ = self.handle_paste(&pasted);
            }
            return Some(ComposerIntent::CopyText(selected));
        }

        if matches_insert_newline_shortcut(key) {
            if let Some(pasted) = self.paste_burst.flush_before_modified_input() {
                let _ = self.handle_paste(&pasted);
            }
            self.apply_textarea_edit(|textarea| textarea.insert_str("\n"));
            self.sync_completion();
            return None;
        }

        if key.modifiers == KeyModifiers::CONTROL {
            if key.code != KeyCode::Char('j')
                && let Some(pasted) = self.paste_burst.flush_before_modified_input()
            {
                let _ = self.handle_paste(&pasted);
            }
            return Some(match key.code {
                KeyCode::Char('d') => ComposerIntent::Exit,
                KeyCode::Char('q') => ComposerIntent::Exit,
                KeyCode::Char('j') => self.submit(),
                KeyCode::Char('x') => {
                    if let Some(cut) = self.textarea.cut_selection() {
                        self.sync_completion();
                        ComposerIntent::CopyText(cut)
                    } else {
                        ComposerIntent::None
                    }
                }
                _ => {
                    self.apply_textarea_edit(|textarea| textarea.handle_key(key));
                    self.sync_completion();
                    ComposerIntent::None
                }
            });
        }

        if is_altgr(key.modifiers) {
            if let Some(pasted) = self.paste_burst.flush_before_modified_input() {
                let _ = self.handle_paste(&pasted);
            }
            self.apply_textarea_edit(|textarea| textarea.handle_key(key));
            self.sync_completion();
            return None;
        }

        if self.completion.is_active() {
            if matches!(key.code, KeyCode::Enter | KeyCode::Tab)
                && let Some(pasted) = self.paste_burst.flush_before_modified_input()
            {
                let _ = self.handle_paste(&pasted);
            }
            match key.code {
                KeyCode::Up => {
                    self.completion.move_up();
                    return Some(ComposerIntent::None);
                }
                KeyCode::Down => {
                    self.completion.move_down();
                    return Some(ComposerIntent::None);
                }
                KeyCode::Esc => {
                    self.completion.clear();
                    return Some(ComposerIntent::None);
                }
                KeyCode::Tab => {
                    self.accept_selected_completion();
                    return Some(ComposerIntent::None);
                }
                KeyCode::Enter => {
                    if let Some(selected) = self.completion.selected().cloned() {
                        if let CompletionSelection::Command(command) = selected.selection {
                            self.textarea.clear();
                            self.completion.clear();
                            return Some(intent_for_slash_command(command, ""));
                        }
                        self.accept_selected_completion();
                        return Some(ComposerIntent::None);
                    }
                }
                _ => {}
            }
        }

        if self.should_handle_history_navigation(key) {
            match key.code {
                KeyCode::Up => {
                    self.navigate_history_up();
                    self.sync_completion();
                    return Some(ComposerIntent::None);
                }
                KeyCode::Down => {
                    self.navigate_history_down();
                    self.sync_completion();
                    return Some(ComposerIntent::None);
                }
                _ => {}
            }
        }

        if matches!(key.code, KeyCode::Enter) && key.modifiers.is_empty() {
            let now = Instant::now();
            if self.paste_burst.is_active() && self.paste_burst.append_newline_if_active(now) {
                return Some(ComposerIntent::None);
            }
            if self
                .paste_burst
                .newline_should_insert_instead_of_submit(now)
            {
                self.apply_textarea_edit(|textarea| textarea.insert_str("\n"));
                self.paste_burst.extend_window(now);
                self.sync_completion();
                return Some(ComposerIntent::None);
            }
        }

        if let KeyEvent {
            code: KeyCode::Char(ch),
            modifiers,
            ..
        } = key
            && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT)
            && !ch.is_ascii_control()
        {
            if ch == '/' {
                if let Some(pasted) = self.paste_burst.flush_before_modified_input() {
                    let _ = self.handle_paste(&pasted);
                }
                self.textarea.handle_key(key);
                self.sync_completion();
                return Some(ComposerIntent::None);
            }

            let now = Instant::now();
            match self.paste_burst.on_plain_char(ch, now) {
                CharDecision::BufferAppend => {
                    self.paste_burst.append_char_to_buffer(ch, now);
                    return Some(ComposerIntent::None);
                }
                CharDecision::BeginBuffer { retro_chars } => {
                    let before_cursor: String = self
                        .textarea
                        .text()
                        .chars()
                        .take(self.textarea.cursor())
                        .collect();
                    if let Some(grab) = self.paste_burst.decide_begin_buffer(
                        now,
                        &before_cursor,
                        retro_chars as usize,
                    ) {
                        if !grab.grabbed.is_empty() {
                            let end = self.textarea.cursor();
                            self.apply_textarea_edit(|textarea| {
                                textarea.replace_char_range(grab.start_char..end, "")
                            });
                        }
                        self.paste_burst.append_char_to_buffer(ch, now);
                        return Some(ComposerIntent::None);
                    }
                }
                CharDecision::BeginBufferFromPending => {
                    self.paste_burst.append_char_to_buffer(ch, now);
                    return Some(ComposerIntent::None);
                }
                CharDecision::RetainFirstChar => return Some(ComposerIntent::None),
            }
        }

        if !matches!(key.code, KeyCode::Char(_) | KeyCode::Enter) {
            if let Some(pasted) = self.paste_burst.flush_before_modified_input() {
                let _ = self.handle_paste(&pasted);
            }
            self.paste_burst.clear_window_after_non_char();
        }

        match key.code {
            KeyCode::Enter => {
                if let Some(pasted) = self.paste_burst.flush_before_modified_input() {
                    let _ = self.handle_paste(&pasted);
                }
                Some(self.submit())
            }
            _ => {
                if key_mutates_text(key) {
                    self.reset_history_navigation_if_needed();
                }
                self.apply_textarea_edit(|textarea| textarea.handle_key(key));
                self.sync_completion();
                None
            }
        }
    }

    pub(crate) fn handle_paste(&mut self, text: &str) -> ComposerIntent {
        self.paste_burst.clear_after_explicit_paste();
        self.reset_history_navigation_if_needed();
        if !self.handle_paste_image_path(text) {
            self.textarea.insert_str(text);
        }
        self.sync_completion();
        ComposerIntent::None
    }

    pub(crate) fn flush_paste_burst_if_due(&mut self) -> bool {
        match self.paste_burst.flush_if_due(Instant::now()) {
            FlushResult::Paste(pasted) => {
                let _ = self.handle_paste(&pasted);
                true
            }
            FlushResult::Typed(ch) => {
                self.textarea.insert_str(&ch.to_string());
                self.sync_completion();
                true
            }
            FlushResult::None => false,
        }
    }

    pub(crate) fn next_paste_flush_delay(&self) -> Option<Duration> {
        self.paste_burst.recommended_flush_delay()
    }

    pub fn render(&self, mode: FrontendMode, width: usize) -> ComposerRender {
        render_composer(
            &self.textarea,
            &self.textarea_state,
            &self.completion,
            mode,
            width,
        )
    }

    pub fn desired_height(&self, mode: FrontendMode, width: usize) -> u16 {
        composer_desired_height(&self.textarea, mode, width)
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    pub fn clear(&mut self) {
        self.textarea.clear();
        *self.textarea_state.borrow_mut() = TextAreaState::default();
        self.completion.clear();
        self.paste_burst.clear_after_explicit_paste();
        self.history.clear_all();
        self.local_images.clear();
        self.remote_images.clear();
        self.attached_skills.clear();
    }

    pub fn restore_submission(&mut self, content: &[InputItem]) {
        self.clear();

        for item in content {
            match item {
                InputItem::Text { text } => {
                    if !self.textarea.is_empty()
                        && self
                            .textarea
                            .text()
                            .chars()
                            .last()
                            .is_some_and(|ch| !ch.is_whitespace() && ch != ']')
                    {
                        self.apply_textarea_edit(|textarea| textarea.insert_str("\n"));
                    }
                    self.apply_textarea_edit(|textarea| textarea.insert_str(text));
                }
                InputItem::Image { source, .. } => match source {
                    AttachmentRef::LocalPath { path } => self.attach_image(PathBuf::from(path)),
                    AttachmentRef::RemoteUrl { url } => self.attach_remote_image(url.clone()),
                    _ => self.append_display_text(&item.display_text()),
                },
                InputItem::File {
                    source,
                    name,
                    mime_type,
                    ..
                } => self.append_display_text(&format_file_restore_text(source, name, mime_type)),
                InputItem::Mention { name, path } => {
                    self.append_display_text(&format!("@{name} ({path})"))
                }
                InputItem::Skill { name, path } => self.attach_skill(name.clone(), path.clone()),
            }
        }

        self.sync_completion();
    }

    pub(crate) fn attach_image(&mut self, path: PathBuf) {
        let placeholder = format!("[Image #{}]", self.total_image_count() + 1);
        if !self.textarea.is_empty()
            && self
                .textarea
                .text()
                .chars()
                .last()
                .is_some_and(|ch| !ch.is_whitespace() && ch != ']')
        {
            self.apply_textarea_edit(|textarea| textarea.insert_str(" "));
        }
        self.apply_textarea_edit(|textarea| textarea.insert_element(&placeholder));
        self.local_images
            .push(LocalAttachedImage { placeholder, path });
        self.sync_completion();
    }

    pub(crate) fn attach_remote_image(&mut self, url: impl Into<String>) {
        let placeholder = format!("[Image #{}]", self.total_image_count() + 1);
        if !self.textarea.is_empty()
            && self
                .textarea
                .text()
                .chars()
                .last()
                .is_some_and(|ch| !ch.is_whitespace() && ch != ']')
        {
            self.apply_textarea_edit(|textarea| textarea.insert_str(" "));
        }
        self.apply_textarea_edit(|textarea| textarea.insert_element(&placeholder));
        self.remote_images.push(RemoteAttachedImage {
            placeholder,
            url: url.into(),
        });
        self.sync_completion();
    }

    pub(crate) fn attach_skill(&mut self, name: String, path: String) {
        let placeholder = format!("${name}");
        if !self.textarea.is_empty()
            && self
                .textarea
                .text()
                .chars()
                .last()
                .is_some_and(|ch| !ch.is_whitespace() && ch != ']')
        {
            self.apply_textarea_edit(|textarea| textarea.insert_str(" "));
        }
        self.apply_textarea_edit(|textarea| textarea.insert_element(&placeholder));
        self.attached_skills.push(AttachedSkill {
            placeholder,
            name,
            path,
        });
        self.sync_completion();
    }

    pub(crate) fn set_available_skills(&mut self, skills: Vec<SkillCompletion>) {
        self.available_skills = skills;
        self.sync_completion();
    }

    #[cfg(test)]
    pub(crate) fn attached_image_paths(&self) -> Vec<PathBuf> {
        self.local_images
            .iter()
            .map(|img| img.path.clone())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn attached_remote_image_urls(&self) -> Vec<String> {
        self.remote_images
            .iter()
            .map(|img| img.url.clone())
            .collect()
    }

    pub fn has_selection(&self) -> bool {
        self.textarea.has_selection()
    }

    pub fn has_completion_menu(&self) -> bool {
        self.completion.is_active()
    }

    pub fn cursor_position(&self, area: Rect, mode: FrontendMode) -> (u16, u16) {
        composer_cursor_position(&self.textarea, &self.textarea_state, area, mode)
    }

    fn submit(&mut self) -> ComposerIntent {
        let raw = self.textarea.text().to_string();
        let leading_space_escape = raw.starts_with(' ');
        let text = self.textarea.take_trimmed();
        let local_images = std::mem::take(&mut self.local_images);
        let remote_images = std::mem::take(&mut self.remote_images);
        self.completion.clear();
        self.history.clear_navigation();
        let attached_skills = std::mem::take(&mut self.attached_skills);
        let content =
            build_submission_content(&text, &local_images, &remote_images, &attached_skills);
        if content.is_empty() {
            ComposerIntent::None
        } else {
            if !text.is_empty() {
                self.history.record(text.clone());
            }
            if local_images.is_empty()
                && remote_images.is_empty()
                && attached_skills.is_empty()
                && !leading_space_escape
                && let Some(command_text) = text.strip_prefix('/')
            {
                let mut parts = command_text.splitn(2, char::is_whitespace);
                let name = parts.next().unwrap_or_default();
                let args = parts.next().unwrap_or_default().trim();
                if let Some(command) = find_slash_command(name)
                    && (args.is_empty() || command.supports_inline_args())
                {
                    return intent_for_slash_command(command, args);
                }
                return ComposerIntent::UnknownCommand(name.to_string());
            }
            ComposerIntent::Submit(content)
        }
    }

    fn sync_completion(&mut self) {
        self.prune_attached_images();
        self.completion.sync_from_input(
            self.textarea.text(),
            self.textarea.byte_cursor(),
            &self.available_skills,
        );
    }

    fn prune_attached_images(&mut self) {
        let present_placeholders = self.textarea.element_payloads();
        let mut retained = Vec::new();

        for image in &self.local_images {
            if !present_placeholders.contains(&image.placeholder) {
                continue;
            }
            retained.push(LocalAttachedImage {
                placeholder: image.placeholder.clone(),
                path: image.path.clone(),
            });
        }
        self.local_images = retained;
        self.remote_images
            .retain(|image| present_placeholders.contains(&image.placeholder));
        self.attached_skills
            .retain(|skill| present_placeholders.contains(&skill.placeholder));
        self.relabel_images_and_update_placeholders();
    }

    fn handle_paste_image_path(&mut self, pasted: &str) -> bool {
        let Some(path) = normalize_pasted_image_path(pasted) else {
            return false;
        };
        if !is_supported_image_path(&path) {
            return false;
        }
        self.attach_image(path);
        true
    }

    fn should_handle_history_navigation(&self, key: KeyEvent) -> bool {
        if !matches!(key.kind, KeyEventKind::Press) {
            return false;
        }
        if key.modifiers != KeyModifiers::NONE {
            return false;
        }
        if !matches!(key.code, KeyCode::Up | KeyCode::Down) {
            return false;
        }
        if self.history.is_empty() {
            return false;
        }

        let text = self.textarea.text();
        if text.is_empty() {
            return true;
        }

        let cursor = self.textarea.cursor();
        if cursor != 0 && cursor != text.chars().count() {
            return false;
        }

        self.history.last_text_matches(text)
    }

    fn navigate_history_up(&mut self) {
        if let Some(entry) = self.history.navigate_up() {
            self.apply_history_entry(entry);
        }
    }

    fn navigate_history_down(&mut self) {
        match self.history.navigate_down() {
            HistoryNavigation::Apply(entry) => self.apply_history_entry(entry),
            HistoryNavigation::ClearComposer => {
                self.textarea.clear();
                *self.textarea_state.borrow_mut() = TextAreaState::default();
            }
            HistoryNavigation::Unchanged => {}
        }
    }

    fn apply_history_entry(&mut self, entry: String) {
        self.textarea.set_text(entry);
        *self.textarea_state.borrow_mut() = TextAreaState::default();
    }

    fn reset_history_navigation_if_needed(&mut self) {
        self.history.clear_navigation();
    }

    fn accept_selected_completion(&mut self) {
        let Some(selected) = self.completion.selected().cloned() else {
            return;
        };
        match selected.selection {
            CompletionSelection::Command(command) => {
                if command.spec().argument_hint.is_some() {
                    self.textarea.set_text(format!("/{} ", command.name()));
                } else {
                    self.textarea.set_text(format!("/{}", command.name()));
                }
            }
            CompletionSelection::FilterValue(value) => {
                if self.textarea.text().starts_with("/filter") {
                    self.textarea.set_text(format!("/filter {value} "));
                }
            }
            CompletionSelection::Skill(skill) => {
                if let Some(range) = self.completion.selected_skill_replace_range() {
                    self.textarea.replace_char_range(range, "");
                    self.attach_skill(skill.name, skill.path);
                    if self
                        .textarea
                        .text()
                        .chars()
                        .last()
                        .is_some_and(|ch| !ch.is_whitespace())
                    {
                        self.apply_textarea_edit(|textarea| textarea.insert_str(" "));
                    }
                }
            }
        }
        self.completion.clear();
    }

    fn apply_textarea_edit(&mut self, edit: impl FnOnce(&mut TextArea)) {
        let elements_before = if self.local_images.is_empty() && self.remote_images.is_empty() {
            None
        } else {
            Some(self.textarea.element_payloads())
        };
        edit(&mut self.textarea);
        if let Some(elements_before) = elements_before {
            self.reconcile_deleted_elements(elements_before);
        }
    }

    fn reconcile_deleted_elements(&mut self, elements_before: Vec<String>) {
        let elements_after = self.textarea.element_payloads();
        let mut removed_any_image = false;
        for removed in elements_before
            .into_iter()
            .filter(|payload| !elements_after.contains(payload))
        {
            if let Some(index) = self
                .local_images
                .iter()
                .position(|image| image.placeholder == removed)
            {
                self.local_images.remove(index);
                removed_any_image = true;
                continue;
            }
            if let Some(index) = self
                .remote_images
                .iter()
                .position(|image| image.placeholder == removed)
            {
                self.remote_images.remove(index);
                removed_any_image = true;
            }
        }

        if removed_any_image {
            self.relabel_images_and_update_placeholders();
        }
    }

    fn relabel_images_and_update_placeholders(&mut self) {
        let mut next_index = 1usize;

        for image in &mut self.remote_images {
            let expected = format!("[Image #{}]", next_index);
            next_index += 1;
            if image.placeholder == expected {
                continue;
            }

            let current = image.placeholder.clone();
            image.placeholder = expected.clone();
            let _ = self.textarea.replace_element_payload(&current, &expected);
        }

        for image in &mut self.local_images {
            let expected = format!("[Image #{}]", next_index);
            next_index += 1;
            if image.placeholder == expected {
                continue;
            }

            let current = image.placeholder.clone();
            image.placeholder = expected.clone();
            let _ = self.textarea.replace_element_payload(&current, &expected);
        }
    }

    fn total_image_count(&self) -> usize {
        self.local_images.len() + self.remote_images.len()
    }

    fn append_display_text(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if !self.textarea.is_empty()
            && self
                .textarea
                .text()
                .chars()
                .last()
                .is_some_and(|ch| !ch.is_whitespace() && ch != ']')
        {
            self.apply_textarea_edit(|textarea| textarea.insert_str("\n"));
        }
        self.apply_textarea_edit(|textarea| textarea.insert_str(text));
    }
}

impl Default for ChatComposer {
    fn default() -> Self {
        Self::new()
    }
}

fn key_mutates_text(key: KeyEvent) -> bool {
    if matches!(
        key.code,
        KeyCode::Backspace | KeyCode::Delete | KeyCode::Tab | KeyCode::Enter
    ) {
        return true;
    }

    match key {
        KeyEvent {
            code: KeyCode::Char(ch),
            modifiers,
            ..
        } if !ch.is_ascii_control()
            && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) =>
        {
            true
        }
        KeyEvent {
            code: KeyCode::Char('h' | 'd' | 'k' | 'u' | 'w' | 'x' | 'y'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => true,
        KeyEvent {
            code:
                KeyCode::Char('d' | 'b' | 'f')
                | KeyCode::Delete
                | KeyCode::Backspace
                | KeyCode::Left
                | KeyCode::Right,
            modifiers: KeyModifiers::ALT,
            ..
        } => true,
        _ => false,
    }
}

fn format_file_restore_text(
    source: &AttachmentRef,
    name: &Option<String>,
    mime_type: &Option<String>,
) -> String {
    let label = name.clone().unwrap_or_else(|| match source {
        AttachmentRef::LocalPath { path } => path.clone(),
        AttachmentRef::RemoteUrl { url } => url.clone(),
        AttachmentRef::HubAsset { asset_id, .. } => format!("hub:{asset_id}"),
        AttachmentRef::InlineDataUrl { .. } => "[inline file]".to_string(),
    });

    match mime_type {
        Some(mime) if !mime.is_empty() => format!("[Attachment: {label} ({mime})]"),
        _ => format!("[Attachment: {label}]"),
    }
}
