use crate::ui::bottom_pane::support::text_effects::shimmer_spans_for_frame;
use crate::ui::theme::{
    body_style, hint_style, title_style, welcome_accent_style, welcome_mascot_style,
    welcome_signal_style,
};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

pub struct WelcomeScreen {
    pub status_text: String,
    pub animation_frame: u64,
}

impl WelcomeScreen {
    pub fn new(status_text: String, animation_frame: u64) -> Self {
        Self {
            status_text,
            animation_frame,
        }
    }

    pub fn render(&self, _area: Rect) -> Paragraph<'static> {
        let dim = hint_style();
        let accent = welcome_accent_style();
        let mascot_style = welcome_mascot_style();
        let soft = body_style();

        let subtitle = self.status_text.clone();

        let mut lines: Vec<Line<'static>> = vec![
            Line::raw(""),
            Line::from(vec![
                Span::raw("      "),
                Span::styled("▄▄▄▄▄▄▄", mascot_style),
            ]),
            Line::from(vec![
                Span::raw("     "),
                Span::styled("█ ", mascot_style),
                Span::styled("●", welcome_signal_style()),
                Span::styled("   ", mascot_style),
                Span::styled("●", welcome_signal_style()),
                Span::styled(" █", mascot_style),
                Span::raw("   "),
                Span::styled(
                    "Hello, I'm CloudAgent",
                    title_style(),
                ),
            ]),
            Line::from(vec![
                Span::raw("     "),
                Span::styled("█   ", mascot_style),
                Span::styled("▄", mascot_style),
                Span::styled("   █", mascot_style),
                Span::raw("   "),
                Span::styled("Your autonomous ops partner", dim),
            ]),
            Line::from(vec![
                Span::raw("      "),
                Span::styled("▀▀▀▀▀▀▀", mascot_style),
            ]),
            Line::raw(""),
        ];

        let title_spans = shimmer_spans_for_frame("CloudAgent", self.animation_frame);

        let mut title_line = vec![Span::raw("  > ")];
        title_line.extend(title_spans);
        lines.push(Line::from(title_line));
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(subtitle, dim),
        ]));
        lines.push(Line::raw(""));

        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("How can I help you today?", accent),
        ]));
        lines.push(Line::raw(""));

        let suggestions = [
            "What's the current disk usage across all partitions?",
            "Help me understand the codebase architecture",
            "Review the recent git commits and summarize changes",
            "Create a bash script to automate database backups",
        ];

        for suggestion in suggestions {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("→ ", hint_style()),
                Span::styled(suggestion.to_string(), soft),
            ]));
        }

        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Enter ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("send  ", dim),
            Span::styled(
                "Ctrl+D ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("exit  ", dim),
        ]));

        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Left)
    }
}

