use anyhow::Result;
use crossterm::cursor::{MoveTo, RestorePosition, SavePosition};
use crossterm::queue;
use crossterm::style::{
    Attribute, Color as CrosstermColor, Print, ResetColor, SetAttribute, SetBackgroundColor,
    SetForegroundColor,
};
use ratatui::Frame;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use std::io::Write;

use crate::terminal::TerminalGuard;

#[allow(dead_code)]
pub(crate) fn draw_frame(
    terminal: &mut TerminalGuard,
    render: impl FnOnce(&mut Frame),
) -> Result<()> {
    terminal.terminal.draw(render)?;
    Ok(())
}

pub(crate) fn draw_spans_at(
    terminal: &mut TerminalGuard,
    x: u16,
    y: u16,
    spans: &[Span<'_>],
) -> Result<()> {
    let backend = terminal.terminal.backend_mut();
    queue!(backend, SavePosition, MoveTo(x, y))?;
    for span in spans {
        queue_style(backend, span.style)?;
        queue!(backend, Print(span.content.as_ref()))?;
        queue!(backend, ResetColor, SetAttribute(Attribute::Reset))?;
    }
    queue!(backend, RestorePosition)?;
    backend.flush()?;
    Ok(())
}

fn queue_style(writer: &mut impl Write, style: Style) -> Result<()> {
    if let Some(fg) = style.fg.and_then(to_crossterm_color) {
        queue!(writer, SetForegroundColor(fg))?;
    }
    if let Some(bg) = style.bg.and_then(to_crossterm_color) {
        queue!(writer, SetBackgroundColor(bg))?;
    }
    if style.add_modifier.contains(Modifier::BOLD) {
        queue!(writer, SetAttribute(Attribute::Bold))?;
    }
    Ok(())
}

fn to_crossterm_color(color: Color) -> Option<CrosstermColor> {
    match color {
        Color::Reset => Some(CrosstermColor::Reset),
        Color::Black => Some(CrosstermColor::Black),
        Color::Red => Some(CrosstermColor::DarkRed),
        Color::Green => Some(CrosstermColor::DarkGreen),
        Color::Yellow => Some(CrosstermColor::DarkYellow),
        Color::Blue => Some(CrosstermColor::DarkBlue),
        Color::Magenta => Some(CrosstermColor::DarkMagenta),
        Color::Cyan => Some(CrosstermColor::DarkCyan),
        Color::Gray => Some(CrosstermColor::Grey),
        Color::DarkGray => Some(CrosstermColor::DarkGrey),
        Color::LightRed => Some(CrosstermColor::Red),
        Color::LightGreen => Some(CrosstermColor::Green),
        Color::LightYellow => Some(CrosstermColor::Yellow),
        Color::LightBlue => Some(CrosstermColor::Blue),
        Color::LightMagenta => Some(CrosstermColor::Magenta),
        Color::LightCyan => Some(CrosstermColor::Cyan),
        Color::White => Some(CrosstermColor::White),
        Color::Rgb(r, g, b) => Some(CrosstermColor::Rgb { r, g, b }),
        Color::Indexed(index) => Some(CrosstermColor::AnsiValue(index)),
    }
}
