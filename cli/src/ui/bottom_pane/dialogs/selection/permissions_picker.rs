use crate::app::commands::permission_profile::{
    DEFAULT_PERMISSION_MODE, PERMISSION_MODE_SPECS, PermissionModeSpec, canonical_permission_mode,
};
use crate::input::intent::ComposerIntent;
use crate::text_width::display_width;
use crate::ui::bottom_pane::bottom_pane_view::{BottomPaneView, BottomPaneViewAction, ViewKind};
use crate::ui::theme::{picker_selected_style, picker_unselected_style};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::text::{Line, Span};

pub struct PermissionsPicker {
    selected: usize,
    options: &'static [PermissionModeSpec],
}

const MAX_VISIBLE_OPTIONS: usize = 5;

impl PermissionsPicker {
    pub fn new(current: &str) -> Self {
        let options = &PERMISSION_MODE_SPECS;
        let selected = options
            .iter()
            .position(|m| m.mode == canonical_permission_mode(current))
            .or_else(|| {
                options
                    .iter()
                    .position(|m| m.mode == DEFAULT_PERMISSION_MODE)
            })
            .unwrap_or(0);
        Self { selected, options }
    }
}

impl BottomPaneView for PermissionsPicker {
    fn kind(&self) -> ViewKind {
        ViewKind::Permissions
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if !matches!(key.kind, KeyEventKind::Press) {
            return BottomPaneViewAction::None;
        }
        match key.code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                BottomPaneViewAction::None
            }
            KeyCode::Down => {
                if self.selected + 1 < self.options.len() {
                    self.selected += 1;
                }
                BottomPaneViewAction::None
            }
            KeyCode::Enter => BottomPaneViewAction::Composer(ComposerIntent::Permissions(
                self.options[self.selected].mode.to_string(),
            )),
            KeyCode::Esc | KeyCode::Char('q') => BottomPaneViewAction::Cancel,
            _ => BottomPaneViewAction::None,
        }
    }

    fn render_lines(&self, area_width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from("  Permissions Picker (session-scoped)"),
            Line::from("  Choose how broad tool execution can be in this session"),
        ];
        let (start, end) = self.visible_window(MAX_VISIBLE_OPTIONS);
        if start > 0 {
            lines.push(Line::from("  ..."));
        }
        for (idx, spec) in self.options[start..end].iter().enumerate() {
            let absolute_idx = start + idx;
            let selected = absolute_idx == self.selected;
            let marker = if selected { "> " } else { "  " };
            let style = if selected {
                picker_selected_style()
            } else {
                picker_unselected_style()
            };
            let mode_col = format!("{marker}{:<14}", spec.mode);
            let mode_text = pad_to_width(&mode_col, 17);
            let available = area_width.saturating_sub(21) as usize;
            let label = truncate_to_width(spec.label, available);
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(mode_text, style),
                Span::styled(label, picker_unselected_style()),
            ]));
        }
        if end < self.options.len() {
            lines.push(Line::from("  ..."));
        }
        lines
    }

    fn desired_height(&self, _area_width: u16) -> u16 {
        let visible = self.options.len().min(MAX_VISIBLE_OPTIONS) as u16;
        4 + visible
            + if self.options.len() > MAX_VISIBLE_OPTIONS {
                1
            } else {
                0
            }
    }
}

impl PermissionsPicker {
    fn visible_window(&self, max_rows: usize) -> (usize, usize) {
        if self.options.is_empty() || max_rows == 0 {
            return (0, 0);
        }
        let visible = self.options.len().min(max_rows);
        let start = if self.selected < visible {
            0
        } else {
            (self.selected + 1).saturating_sub(visible)
        }
        .min(self.options.len().saturating_sub(visible));
        (start, start + visible)
    }
}

fn truncate_to_width(value: &str, width: usize) -> String {
    if width == 0 || display_width(value) <= width {
        return value.to_string();
    }
    let mut out = String::new();
    for ch in value.chars() {
        let next = format!("{out}{ch}");
        if display_width(&next) + 3 > width {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn pad_to_width(value: &str, width: usize) -> String {
    let mut out = value.to_string();
    let current = display_width(&out);
    if current < width {
        out.push_str(&" ".repeat(width - current));
    }
    out
}
