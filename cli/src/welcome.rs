use crate::history_cell::shimmer_spans;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

pub struct WelcomeScreen {
    pub history_loaded: bool,
    pub status_text: String,
}

impl WelcomeScreen {
    pub fn new(history_loaded: bool, status_text: String) -> Self {
        Self {
            history_loaded,
            status_text,
        }
    }

    pub fn render(&self, _area: Rect) -> Paragraph<'static> {
        let logo_color = Color::Rgb(100, 140, 255);
        let dim = Color::Rgb(70, 70, 90);
        let accent = Color::Rgb(140, 160, 230);
        let mascot_color = Color::Rgb(120, 130, 200);
        let soft = Color::Rgb(120, 120, 145);

        let subtitle = if self.history_loaded {
            match self.status_text.as_str() {
                "Loaded history" => "Workspace context ready".to_string(),
                other => other.to_string(),
            }
        } else {
            "Loading your workspace context...".to_string()
        };

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::raw(""));

        lines.push(Line::from(vec![
            Span::raw("      "),
            Span::styled("▄▄▄▄▄▄▄", Style::default().fg(mascot_color)),
        ]));
        lines.push(Line::from(vec![
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
        ]));
        lines.push(Line::from(vec![
            Span::raw("     "),
            Span::styled("█   ", Style::default().fg(mascot_color)),
            Span::styled("▄", Style::default().fg(mascot_color)),
            Span::styled("   █", Style::default().fg(mascot_color)),
            Span::raw("   "),
            Span::styled("Your autonomous ops partner", Style::default().fg(dim)),
        ]));
        lines.push(Line::from(vec![
            Span::raw("      "),
            Span::styled("▀▀▀▀▀▀▀", Style::default().fg(mascot_color)),
        ]));
        lines.push(Line::raw(""));

        let title_spans = if self.history_loaded {
            shimmer_spans("CloudAgent")
        } else {
            vec![Span::styled(
                "CloudAgent",
                Style::default().fg(logo_color).add_modifier(Modifier::BOLD),
            )]
        };

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
                "Ctrl+K ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("interrupt  ", Style::default().fg(dim)),
            Span::styled(
                "F2 ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("history  ", Style::default().fg(dim)),
            Span::styled(
                "F4 ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("clear", Style::default().fg(dim)),
        ]));

        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Left)
    }
}
