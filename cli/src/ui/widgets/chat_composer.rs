use crate::input::completion::CompletionState;
use crate::input::intent::ComposerIntent;
use crate::input::slash_command::{SlashCommand, find_slash_command};
use crate::text_width::display_width;
use crate::ui::widgets::completion_popup::completion_popup_lines;
use crate::ui::widgets::paste_burst::{CharDecision, FlushResult, PasteBurst};
use agent_core::conversation::{AttachmentRef, InputItem};
use agent_protocol::FrontendMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Instant;

use crate::app::clipboard_paste::{is_supported_image_path, normalize_pasted_image_path};
use crate::ui::widgets::textarea::{TextArea, TextAreaState, is_altgr};

pub struct ComposerRender {
    pub lines: Vec<Line<'static>>,
    pub completion_lines: Vec<Line<'static>>,
    pub cursor_row: u16,
    pub height: u16,
}

const MAX_VISIBLE_COMPOSER_ROWS: usize = 8;

struct ComposerLayout {
    prompt_prefix: String,
    prompt_width: usize,
    content_width: usize,
}

pub struct ChatComposer {
    textarea: TextArea,
    textarea_state: RefCell<TextAreaState>,
    completion: CompletionState,
    paste_burst: PasteBurst,
    history: Vec<String>,
    history_cursor: Option<usize>,
    last_history_text: Option<String>,
    local_images: Vec<LocalAttachedImage>,
    remote_images: Vec<RemoteAttachedImage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LocalAttachedImage {
    placeholder: String,
    path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RemoteAttachedImage {
    placeholder: String,
    url: String,
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            textarea: TextArea::new(),
            textarea_state: RefCell::new(TextAreaState::default()),
            completion: CompletionState::default(),
            paste_burst: PasteBurst::default(),
            history: Vec::new(),
            history_cursor: None,
            last_history_text: None,
            local_images: Vec::new(),
            remote_images: Vec::new(),
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

        if key.code == KeyCode::Enter && is_newline_shortcut(key.modifiers) {
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
                    if let Some(selected) = self.completion.selected() {
                        if let Some(command) = selected.command {
                            self.textarea.clear();
                            self.completion.clear();
                            return Some(action_for_command(command, ""));
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

    pub fn render(&self, mode: FrontendMode, width: usize) -> ComposerRender {
        let (prompt_color, prompt_bg) = match mode {
            FrontendMode::WaitingForServerRequest => (Color::Rgb(255, 184, 76), None),
            FrontendMode::Running => (Color::Rgb(100, 160, 255), None),
            FrontendMode::Idle => (Color::Rgb(150, 180, 255), None),
        };
        let layout = self.layout(mode, width);

        let body = if self.textarea.is_empty() {
            match mode {
                FrontendMode::Idle => "Ask anything — e.g. \"check disk pressure\"",
                FrontendMode::WaitingForServerRequest => "Type y / n, or enter a short reason",
                FrontendMode::Running => "",
            }
        } else {
            self.textarea.text()
        };

        let full_height = if self.textarea.is_empty() {
            self.textarea
                .wrapped_lines(body, layout.content_width)
                .len() as u16
        } else {
            self.textarea.desired_height(layout.content_width)
        };
        let is_placeholder = self.textarea.is_empty();
        let mut lines = Vec::new();
        let visible_height = full_height.clamp(1, MAX_VISIBLE_COMPOSER_ROWS as u16);
        let (visible_lines, cursor_row, scroll_top) = if self.textarea.is_empty() {
            let wrapped = self.textarea.wrapped_lines(body, layout.content_width);
            let visible_height_usize = visible_height as usize;
            let scroll_top = wrapped.len().saturating_sub(visible_height_usize);
            let cursor_row = wrapped.len().saturating_sub(scroll_top).saturating_sub(1) as u16;
            (
                wrapped
                    .into_iter()
                    .skip(scroll_top)
                    .take(visible_height_usize)
                    .collect::<Vec<_>>(),
                cursor_row,
                scroll_top,
            )
        } else {
            let mut state = self.textarea_state.borrow_mut();
            let visible_lines = self.textarea.visible_wrapped_lines(
                body,
                layout.content_width,
                visible_height,
                &mut state,
            );
            let (cursor_row, _) = self.textarea.visual_cursor_position_with_state(
                layout.content_width,
                visible_height,
                &mut state,
            );
            (visible_lines, cursor_row as u16, state.scroll as usize)
        };

        for (visible_index, wrapped_line) in visible_lines.into_iter().enumerate() {
            let actual_index = scroll_top + visible_index;
            let indent = if actual_index == 0 {
                layout.prompt_prefix.clone()
            } else {
                " ".repeat(layout.prompt_width)
            };
            let prompt_style = {
                let base = Style::default()
                    .fg(prompt_color)
                    .add_modifier(Modifier::BOLD);
                if actual_index == 0 {
                    prompt_bg.map_or(base, |bg| base.bg(bg))
                } else {
                    Style::default().fg(Color::Rgb(55, 55, 68))
                }
            };
            lines.push(Line::from(vec![
                Span::styled(indent, prompt_style),
                Span::styled(
                    wrapped_line,
                    if is_placeholder {
                        Style::default().fg(Color::Rgb(65, 65, 80))
                    } else if self.textarea.is_all_selected() {
                        Style::default()
                            .fg(Color::Rgb(40, 40, 52))
                            .bg(Color::Rgb(220, 220, 230))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(220, 220, 230))
                    },
                ),
            ]));
        }
        let completion_lines = completion_popup_lines(&self.completion, width, layout.prompt_width);

        ComposerRender {
            lines,
            completion_lines,
            cursor_row,
            height: visible_height,
        }
    }

    pub fn desired_height(&self, mode: FrontendMode, width: usize) -> u16 {
        let layout = self.layout(mode, width);
        let body = if self.textarea.is_empty() {
            match mode {
                FrontendMode::Idle => "Ask anything — e.g. \"check disk pressure\"",
                FrontendMode::WaitingForServerRequest => "Type y / n, or enter a short reason",
                FrontendMode::Running => "",
            }
        } else {
            self.textarea.text()
        };
        self.textarea
            .wrapped_lines(body, layout.content_width)
            .len()
            .clamp(1, MAX_VISIBLE_COMPOSER_ROWS) as u16
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    pub fn clear(&mut self) {
        self.textarea.clear();
        *self.textarea_state.borrow_mut() = TextAreaState::default();
        self.completion.clear();
        self.paste_burst.clear_after_explicit_paste();
        self.history_cursor = None;
        self.last_history_text = None;
        self.local_images.clear();
        self.remote_images.clear();
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
                InputItem::Skill { name, path } => {
                    self.append_display_text(&format!("#{name} ({path})"))
                }
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

    #[allow(dead_code)]
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
        let layout = self.layout(mode, area.width as usize);
        let visible_height = self.desired_height(mode, area.width as usize);
        let (cursor_row, cursor_col) = if self.textarea.is_empty() {
            (0, 0)
        } else {
            let mut state = self.textarea_state.borrow_mut();
            self.textarea.visual_cursor_position_with_state(
                layout.content_width,
                visible_height,
                &mut state,
            )
        };
        let max_x_offset = area.width.saturating_sub(1) as usize;
        let x = area.x + (layout.prompt_width + cursor_col).min(max_x_offset) as u16;
        let y = area.y + cursor_row as u16;
        (x, y)
    }

    fn layout(&self, mode: FrontendMode, width: usize) -> ComposerLayout {
        let prompt_text = match mode {
            FrontendMode::WaitingForServerRequest => "?",
            FrontendMode::Running | FrontendMode::Idle => ">",
        };
        let prompt_prefix = format!("  {prompt_text} ");
        let prompt_width = display_width(&prompt_prefix);
        let content_width = width.saturating_sub(prompt_width + 2).max(10);
        ComposerLayout {
            prompt_prefix,
            prompt_width,
            content_width,
        }
    }

    fn submit(&mut self) -> ComposerIntent {
        let raw = self.textarea.text().to_string();
        let leading_space_escape = raw.starts_with(' ');
        let text = self.textarea.take_trimmed();
        let local_images = std::mem::take(&mut self.local_images);
        let remote_images = std::mem::take(&mut self.remote_images);
        self.completion.clear();
        self.history_cursor = None;
        self.last_history_text = None;
        let content = build_submission_content(&text, &local_images, &remote_images);
        if content.is_empty() {
            ComposerIntent::None
        } else {
            if !text.is_empty() {
                self.record_history_entry(text.clone());
            }
            if local_images.is_empty()
                && remote_images.is_empty()
                && !leading_space_escape
                && let Some(command_text) = text.strip_prefix('/')
            {
                let mut parts = command_text.splitn(2, char::is_whitespace);
                let name = parts.next().unwrap_or_default();
                let args = parts.next().unwrap_or_default().trim();
                if let Some(command) = find_slash_command(name)
                    && (args.is_empty() || command.supports_inline_args())
                {
                    return action_for_command(command, args);
                }
                return ComposerIntent::UnknownCommand(name.to_string());
            }
            ComposerIntent::Submit(content)
        }
    }

    fn sync_completion(&mut self) {
        self.prune_attached_images();
        self.completion
            .sync_from_input(self.textarea.text(), self.textarea.byte_cursor());
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

        matches!(&self.last_history_text, Some(prev) if prev == text)
    }

    fn navigate_history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_cursor {
            None => self.history.len().saturating_sub(1),
            Some(0) => 0,
            Some(idx) => idx.saturating_sub(1),
        };
        self.history_cursor = Some(next);
        self.apply_history_entry(next);
    }

    fn navigate_history_down(&mut self) {
        let Some(idx) = self.history_cursor else {
            return;
        };
        if idx + 1 >= self.history.len() {
            self.history_cursor = None;
            self.last_history_text = None;
            self.textarea.clear();
            *self.textarea_state.borrow_mut() = TextAreaState::default();
            return;
        }
        let next = idx + 1;
        self.history_cursor = Some(next);
        self.apply_history_entry(next);
    }

    fn apply_history_entry(&mut self, index: usize) {
        if let Some(entry) = self.history.get(index).cloned() {
            self.textarea.set_text(entry.clone());
            *self.textarea_state.borrow_mut() = TextAreaState::default();
            self.last_history_text = Some(entry);
        }
    }

    fn reset_history_navigation_if_needed(&mut self) {
        self.history_cursor = None;
        self.last_history_text = None;
    }

    fn record_history_entry(&mut self, entry: String) {
        if self.history.last().is_some_and(|last| last == &entry) {
            return;
        }
        self.history.push(entry);
    }

    fn accept_selected_completion(&mut self) {
        let Some(selected) = self.completion.selected() else {
            return;
        };
        if let Some(command) = selected.command {
            if command.spec().argument_hint.is_some() {
                self.textarea.set_text(format!("/{} ", command.name()));
            } else {
                self.textarea.set_text(format!("/{}", command.name()));
            }
        } else if self.textarea.text().starts_with("/filter") {
            self.textarea
                .set_text(format!("/filter {} ", selected.insertion));
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

fn is_newline_shortcut(modifiers: KeyModifiers) -> bool {
    let shift_only = modifiers.contains(KeyModifiers::SHIFT)
        && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    shift_only || modifiers == KeyModifiers::ALT || modifiers == KeyModifiers::CONTROL
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

enum PendingImage<'a> {
    Local(&'a LocalAttachedImage),
    Remote(&'a RemoteAttachedImage),
}

impl PendingImage<'_> {
    fn placeholder(&self) -> &str {
        match self {
            Self::Local(image) => &image.placeholder,
            Self::Remote(image) => &image.placeholder,
        }
    }

    fn to_input_item(&self) -> InputItem {
        match self {
            Self::Local(image) => InputItem::Image {
                source: AttachmentRef::LocalPath {
                    path: image.path.display().to_string(),
                },
                detail: None,
                alt: None,
            },
            Self::Remote(image) => InputItem::Image {
                source: AttachmentRef::RemoteUrl {
                    url: image.url.clone(),
                },
                detail: None,
                alt: None,
            },
        }
    }
}

fn build_submission_content(
    text: &str,
    local_images: &[LocalAttachedImage],
    remote_images: &[RemoteAttachedImage],
) -> Vec<InputItem> {
    let mut content = Vec::new();
    let mut remaining = text;
    let mut images = local_images
        .iter()
        .map(PendingImage::Local)
        .chain(remote_images.iter().map(PendingImage::Remote))
        .filter(|image| text.contains(image.placeholder()))
        .collect::<Vec<_>>();

    while !images.is_empty() {
        let next = images
            .iter()
            .enumerate()
            .filter_map(|(idx, image)| {
                remaining
                    .find(image.placeholder())
                    .map(|offset| (idx, offset))
            })
            .min_by_key(|(_, offset)| *offset);

        let Some((image_idx, offset)) = next else {
            break;
        };
        let image = images.remove(image_idx);
        let (before, rest) = remaining.split_at(offset);
        push_text_item(&mut content, before);
        content.push(image.to_input_item());
        remaining = &rest[image.placeholder().len()..];
    }

    push_text_item(&mut content, remaining);
    content
}

fn push_text_item(content: &mut Vec<InputItem>, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    content.push(InputItem::Text {
        text: text.trim().to_string(),
    });
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

fn action_for_command(command: SlashCommand, args: &str) -> ComposerIntent {
    match command {
        SlashCommand::Clear => ComposerIntent::Reset,
        SlashCommand::Compact => ComposerIntent::Compact,
        SlashCommand::Copy => ComposerIntent::Copy,
        SlashCommand::Help => ComposerIntent::Help,
        SlashCommand::Interrupt => ComposerIntent::Interrupt,
        SlashCommand::Session => {
            let trimmed = args.trim();
            if trimmed.is_empty() {
                ComposerIntent::Session
            } else {
                ComposerIntent::SessionSwitch(trimmed.to_string())
            }
        }
        SlashCommand::NewConversation => ComposerIntent::NewConversation(args.trim().to_string()),
        SlashCommand::SetTitle => ComposerIntent::SetTitle(args.trim().to_string()),
        SlashCommand::ArchiveConversation => {
            ComposerIntent::ArchiveConversation(args.trim().to_string())
        }
        SlashCommand::DeleteConversation => {
            ComposerIntent::DeleteConversation(args.trim().to_string())
        }
        SlashCommand::Filter => ComposerIntent::Filter(args.trim().to_string()),
        SlashCommand::Permissions => ComposerIntent::Permissions(args.trim().to_string()),
        SlashCommand::Config => ComposerIntent::Config,
        SlashCommand::Gateway => ComposerIntent::Gateway,
        SlashCommand::WeixinLogin => ComposerIntent::WeixinLogin,
        SlashCommand::WeixinLoginCheck => {
            ComposerIntent::WeixinLoginCheck(args.trim().to_string())
        }
        SlashCommand::Exit => ComposerIntent::Exit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn text_items(text: &str) -> Vec<InputItem> {
        vec![InputItem::Text {
            text: text.to_string(),
        }]
    }

    fn create_test_png() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("cloudagent-test-{nonce}.png"));
        image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))
            .save(&path)
            .expect("save temp image");
        path
    }

    fn image_path_string(path: &Path) -> String {
        path.display().to_string()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_text(composer: &mut ChatComposer, text: &str) {
        for ch in text.chars() {
            composer.handle_key(key(KeyCode::Char(ch)));
            std::thread::sleep(std::time::Duration::from_millis(40));
            let _ = composer.flush_paste_burst_if_due();
        }
    }

    #[test]
    fn slash_opens_completion_and_tab_completes() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "/co");

        assert!(composer.completion.is_active());
        composer.handle_key(key(KeyCode::Tab));
        assert_eq!(composer.textarea.text(), "/copy");
    }

    #[test]
    fn enter_dispatches_selected_completion() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "/co");

        let action = composer.handle_key(key(KeyCode::Enter));
        assert_eq!(action, Some(ComposerIntent::Copy));
        assert!(composer.textarea.is_empty());
    }

    #[test]
    fn exact_slash_command_dispatches_without_reducer_parsing() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "/clear");

        let action = composer.handle_key(key(KeyCode::Enter));
        assert_eq!(action, Some(ComposerIntent::Reset));
    }

    #[test]
    fn leading_space_slash_submits_as_message() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, " /clear");

        let action = composer.handle_key(key(KeyCode::Enter));
        assert_eq!(action, Some(ComposerIntent::Submit(text_items("/clear"))));
    }

    #[test]
    fn completion_popup_does_not_shift_cursor() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "/co");

        let (_, y) = composer.cursor_position(Rect::new(0, 10, 80, 5), FrontendMode::Idle);
        assert_eq!(y, 10);
    }

    #[test]
    fn completion_popup_scrolls_to_selected_command() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "/");
        for _ in 0..4 {
            composer.handle_key(key(KeyCode::Down));
        }

        let rendered = composer.render(FrontendMode::Idle, 80);
        let visible_text = rendered
            .completion_lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(visible_text.contains("> /"));
    }

    #[test]
    fn bracketed_paste_inserts_text_without_submitting() {
        let mut composer = ChatComposer::new();

        let action = composer.handle_paste("first line\nsecond line");

        assert_eq!(action, ComposerIntent::None);
        assert_eq!(composer.textarea.text(), "first line\nsecond line");
    }

    #[test]
    fn bracketed_paste_only_submits_after_explicit_enter() {
        let mut composer = ChatComposer::new();

        let paste_action = composer.handle_paste("first line\nsecond line");
        let submit_action = composer.handle_key(key(KeyCode::Enter));

        assert_eq!(paste_action, ComposerIntent::None);
        assert_eq!(
            submit_action,
            Some(ComposerIntent::Submit(text_items(
                "first line\nsecond line"
            )))
        );
        assert!(composer.textarea.is_empty());
    }

    #[test]
    fn trailing_space_remains_visible_in_rendered_composer() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "abc ");

        let rendered = composer.render(FrontendMode::Idle, 80);
        let visible_text = rendered.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(
            visible_text.ends_with("abc "),
            "expected rendered composer to preserve trailing space, got {visible_text:?}"
        );
    }

    #[test]
    fn trailing_space_wraps_into_continuation_row() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "abc ");

        let visible_lines = composer.textarea.wrapped_lines(composer.textarea.text(), 3);

        assert_eq!(visible_lines.len(), 2);
        assert_eq!(visible_lines[0], "abc");
        assert_eq!(visible_lines[1], " ");
    }

    #[test]
    fn long_multiline_input_caps_visible_height() {
        let mut composer = ChatComposer::new();
        let text = (1..=20)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = composer.handle_paste(&text);

        let rendered = composer.render(FrontendMode::Idle, 80);

        assert_eq!(rendered.height, MAX_VISIBLE_COMPOSER_ROWS as u16);
        assert_eq!(rendered.lines.len(), MAX_VISIBLE_COMPOSER_ROWS);
        assert_eq!(rendered.cursor_row, MAX_VISIBLE_COMPOSER_ROWS as u16 - 1);
    }

    #[test]
    fn shift_enter_inserts_newline_without_submitting() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "first");

        let action = composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        type_text(&mut composer, "second");

        assert_eq!(action, None);
        assert_eq!(composer.textarea.text(), "first\nsecond");
    }

    #[test]
    fn alt_enter_inserts_newline_without_submitting() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "first");

        let action = composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        type_text(&mut composer, "second");

        assert_eq!(action, None);
        assert_eq!(composer.textarea.text(), "first\nsecond");
    }

    #[test]
    fn ctrl_enter_inserts_newline_without_submitting() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "first");

        let action = composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        type_text(&mut composer, "second");

        assert_eq!(action, None);
        assert_eq!(composer.textarea.text(), "first\nsecond");
    }

    #[test]
    fn manual_newline_shortcut_submits_multiline_text_only_after_plain_enter() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "first");

        let newline_action =
            composer.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        type_text(&mut composer, "second");
        let submit_action = composer.handle_key(key(KeyCode::Enter));

        assert_eq!(newline_action, None);
        assert_eq!(
            submit_action,
            Some(ComposerIntent::Submit(text_items("first\nsecond")))
        );
        assert!(composer.textarea.is_empty());
    }

    #[test]
    fn pasted_image_path_attaches_placeholder_without_inserting_raw_path() {
        let mut composer = ChatComposer::new();
        let image_path = create_test_png();

        let action = composer.handle_paste(&image_path_string(&image_path));

        assert_eq!(action, ComposerIntent::None);
        assert_eq!(composer.textarea.text(), "[Image #1]");
        assert_eq!(composer.attached_image_paths(), vec![image_path]);
    }

    #[test]
    fn submit_preserves_text_and_image_order() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "look at this");
        let image_path = create_test_png();
        composer.attach_image(image_path.clone());
        type_text(&mut composer, "please");

        let action = composer.handle_key(key(KeyCode::Enter));

        assert_eq!(
            action,
            Some(ComposerIntent::Submit(vec![
                InputItem::Text {
                    text: "look at this".to_string(),
                },
                InputItem::Image {
                    source: AttachmentRef::LocalPath {
                        path: image_path_string(&image_path),
                    },
                    detail: None,
                    alt: None,
                },
                InputItem::Text {
                    text: "please".to_string(),
                },
            ]))
        );
    }

    #[test]
    fn deleting_first_placeholder_renumbers_remaining_images() {
        let mut composer = ChatComposer::new();
        let first = create_test_png();
        let second = create_test_png();

        composer.attach_image(first);
        composer.attach_image(second.clone());
        composer.textarea.handle_key(key(KeyCode::Home));
        composer.handle_key(key(KeyCode::Delete));

        assert_eq!(composer.textarea.text(), "[Image #1]");
        assert_eq!(composer.attached_image_paths(), vec![second]);
    }

    #[test]
    fn backspace_deletes_image_placeholder_atomically() {
        let mut composer = ChatComposer::new();
        let image_path = create_test_png();
        composer.attach_image(image_path);

        composer.handle_key(key(KeyCode::Backspace));

        assert!(composer.textarea.is_empty());
        assert!(composer.attached_image_paths().is_empty());
    }

    #[test]
    fn delete_deletes_image_placeholder_atomically() {
        let mut composer = ChatComposer::new();
        let image_path = create_test_png();
        composer.attach_image(image_path);
        composer.textarea.handle_key(key(KeyCode::Home));

        composer.handle_key(key(KeyCode::Delete));

        assert!(composer.textarea.is_empty());
        assert!(composer.attached_image_paths().is_empty());
    }

    #[test]
    fn local_and_remote_images_share_numbering_and_submit_order() {
        let mut composer = ChatComposer::new();
        let local_path = create_test_png();
        composer.attach_remote_image("https://example.com/a.png");
        composer.attach_image(local_path.clone());
        type_text(&mut composer, "describe both");

        assert_eq!(
            composer.textarea.text(),
            "[Image #1][Image #2]describe both"
        );

        let action = composer.handle_key(key(KeyCode::Enter));

        assert_eq!(
            action,
            Some(ComposerIntent::Submit(vec![
                InputItem::Image {
                    source: AttachmentRef::RemoteUrl {
                        url: "https://example.com/a.png".to_string(),
                    },
                    detail: None,
                    alt: None,
                },
                InputItem::Image {
                    source: AttachmentRef::LocalPath {
                        path: image_path_string(&local_path),
                    },
                    detail: None,
                    alt: None,
                },
                InputItem::Text {
                    text: "describe both".to_string(),
                },
            ]))
        );
    }

    #[test]
    fn deleting_remote_image_relabels_local_images() {
        let mut composer = ChatComposer::new();
        let local_path = create_test_png();
        composer.attach_remote_image("https://example.com/a.png");
        composer.attach_image(local_path.clone());
        composer.textarea.handle_key(key(KeyCode::Home));

        composer.handle_key(key(KeyCode::Delete));

        assert_eq!(composer.textarea.text(), "[Image #1]");
        assert_eq!(composer.attached_remote_image_urls(), Vec::<String>::new());
        assert_eq!(composer.attached_image_paths(), vec![local_path]);
    }

    #[test]
    fn ctrl_a_selects_all_current_draft() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "alpha\nbeta");

        composer.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(
            composer.textarea.selected_text().as_deref(),
            Some("alpha\nbeta")
        );
    }

    #[test]
    fn ctrl_a_then_ctrl_x_cuts_entire_draft_and_returns_copy_intent() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "alpha\nbeta");

        composer.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        let action = composer.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));

        assert_eq!(
            action,
            Some(ComposerIntent::CopyText("alpha\nbeta".to_string()))
        );
        assert!(composer.textarea.is_empty());
    }

    #[test]
    fn ctrl_d_still_exits_even_with_existing_text() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "alpha");

        let action = composer.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));

        assert_eq!(action, Some(ComposerIntent::Exit));
        assert_eq!(composer.textarea.text(), "alpha");
    }

    #[test]
    fn esc_with_existing_text_is_not_consumed_by_composer() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "alpha");

        let action = composer.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(action, None);
        assert_eq!(composer.textarea.text(), "alpha");
    }

    #[test]
    fn placeholder_cursor_stays_at_input_start() {
        let composer = ChatComposer::new();
        let (x, y) = composer.cursor_position(Rect::new(0, 10, 80, 5), FrontendMode::Idle);
        assert_eq!(y, 10);
        assert_eq!(x, 4);
    }

    #[test]
    fn down_moves_to_next_line_and_then_end_of_text() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "first\nsecond");

        composer.handle_key(key(KeyCode::Home));
        composer.handle_key(key(KeyCode::Down));
        assert_eq!(
            composer
                .textarea
                .text()
                .chars()
                .take(composer.textarea.cursor())
                .collect::<String>(),
            "first\n"
        );

        composer.handle_key(key(KeyCode::Down));
        assert_eq!(
            composer.textarea.cursor(),
            composer.textarea.text().chars().count()
        );
    }

    #[test]
    fn history_navigation_only_activates_for_empty_or_recalled_boundary_text() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "first");
        let _ = composer.handle_key(key(KeyCode::Enter));
        type_text(&mut composer, "second");
        let _ = composer.handle_key(key(KeyCode::Enter));

        composer.handle_key(key(KeyCode::Up));
        assert_eq!(composer.textarea.text(), "second");

        composer.handle_key(key(KeyCode::Home));
        composer.handle_key(key(KeyCode::Up));
        assert_eq!(composer.textarea.text(), "first");

        composer.handle_key(key(KeyCode::Down));
        assert_eq!(composer.textarea.text(), "second");

        composer.handle_key(key(KeyCode::End));
        type_text(&mut composer, "!");
        composer.handle_key(key(KeyCode::Up));
        assert_eq!(composer.textarea.text(), "second!");
    }

    #[test]
    fn down_past_newest_history_clears_composer() {
        let mut composer = ChatComposer::new();
        type_text(&mut composer, "first");
        let _ = composer.handle_key(key(KeyCode::Enter));

        composer.handle_key(key(KeyCode::Up));
        assert_eq!(composer.textarea.text(), "first");

        composer.handle_key(key(KeyCode::Down));
        assert!(composer.textarea.is_empty());
    }

    #[test]
    fn single_plain_char_is_flushed_on_tick() {
        let mut composer = ChatComposer::new();

        let action = composer.handle_key(key(KeyCode::Char('a')));
        assert_eq!(action, Some(ComposerIntent::None));
        assert_eq!(composer.textarea.text(), "");

        std::thread::sleep(std::time::Duration::from_millis(40));
        assert!(composer.flush_paste_burst_if_due());
        assert_eq!(composer.textarea.text(), "a");
    }

    #[test]
    fn two_fast_chars_flush_as_paste() {
        let mut composer = ChatComposer::new();

        let _ = composer.handle_key(key(KeyCode::Char('a')));
        let _ = composer.handle_key(key(KeyCode::Char('b')));
        assert_eq!(composer.textarea.text(), "");

        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(composer.flush_paste_burst_if_due());
        assert_eq!(composer.textarea.text(), "ab");
    }

    #[test]
    fn enter_during_paste_burst_does_not_submit_multiline_text() {
        let mut composer = ChatComposer::new();

        let _ = composer.handle_key(key(KeyCode::Char('a')));
        let _ = composer.handle_key(key(KeyCode::Char('b')));
        let action = composer.handle_key(key(KeyCode::Enter));

        assert_eq!(action, Some(ComposerIntent::None));
        assert_eq!(composer.textarea.text(), "");

        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(composer.flush_paste_burst_if_due());
        assert_eq!(composer.textarea.text(), "ab\n");
    }
}
