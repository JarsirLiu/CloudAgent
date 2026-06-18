use crate::ui::widgets::text_effects::shimmer_spans_for_frame;
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
        let dim = Color::Rgb(70, 70, 90);
        let accent = Color::Rgb(140, 160, 230);
        let mascot_color = Color::Rgb(120, 130, 200);
        let soft = Color::Rgb(120, 120, 145);

        let subtitle = self.status_text.clone();

        let mut lines: Vec<Line<'static>> = vec![
            Line::raw(""),
            Line::from(vec![
                Span::raw("      "),
                Span::styled("▄▄▄▄▄▄▄", Style::default().fg(mascot_color)),
            ]),
            Line::from(vec![
                Span::raw("     "),
                Span::styled("█ ", Style::default().fg(mascot_color)),
                Span::styled("●", Style::default().fg(Color::Rgb(100, 255, 100))),
                Span::styled("   ", Style::default().fg(mascot_color)),
                Span::styled("●", Style::default().fg(Color::Rgb(100, 255, 100))),
                Span::styled(" █", Style::default().fg(mascot_color)),
                Span::raw("   "),
                Span::styled(
                    "Hello, I'm CloudAgent",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::raw("     "),
                Span::styled("█   ", Style::default().fg(mascot_color)),
                Span::styled("▄", Style::default().fg(mascot_color)),
                Span::styled("   █", Style::default().fg(mascot_color)),
                Span::raw("   "),
                Span::styled("Your autonomous ops partner", Style::default().fg(dim)),
            ]),
            Line::from(vec![
                Span::raw("      "),
                Span::styled("▀▀▀▀▀▀▀", Style::default().fg(mascot_color)),
            ]),
            Line::raw(""),
        ];

        let title_spans = shimmer_spans_for_frame("CloudAgent", self.animation_frame);

        let mut title_line = vec![Span::raw("  > ")];
        title_line.extend(title_spans);
        lines.push(Line::from(title_line));
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(subtitle, Style::default().fg(dim)),
        ]));
        lines.push(Line::raw(""));

        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "How can I help you today?",
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
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
                Span::styled("→ ", Style::default().fg(Color::Rgb(90, 100, 135))),
                Span::styled(suggestion.to_string(), Style::default().fg(soft)),
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
            Span::styled("send  ", Style::default().fg(dim)),
            Span::styled(
                "Ctrl+D ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("exit  ", Style::default().fg(dim)),
        ]));

        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Left)
    }
}
