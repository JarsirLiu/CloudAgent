use crate::input::slash_command::SlashCommand;
use crate::text_width::display_width;
use crate::ui::theme::{picker_selected_style, title_style};
use crate::ui::bottom_pane::bottom_pane_view::{BottomPaneView, ViewKind};
use ratatui::text::{Line, Span};

pub struct HelpView;

impl HelpView {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HelpView {
    fn default() -> Self {
        Self::new()
    }
}

impl BottomPaneView for HelpView {
    fn kind(&self) -> ViewKind {
        ViewKind::Help
    }

    fn render_lines(&self, area_width: u16) -> Vec<Line<'static>> {
        let content_width = area_width.saturating_sub(4) as usize;
        let mut lines = vec![
            Line::from(vec![Span::styled("  Command Help", title_style())]),
            Line::from("  Local slash commands:"),
        ];

        for spec in SlashCommand::all() {
            let command = match spec.argument_hint {
                Some(hint) => format!("/{} {}", spec.name, hint),
                None => format!("/{}", spec.name),
            };
            let row = format!("{command:<20} {}", spec.description);
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(truncate_to_width(&row, content_width), picker_selected_style()),
            ]));
        }

        lines.push(Line::from("  Esc to close"));
        lines
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

