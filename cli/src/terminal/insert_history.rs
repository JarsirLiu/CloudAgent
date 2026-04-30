use std::fmt;
use std::io;
use std::io::Write;

use anyhow::Result;
use crossterm::cursor::{MoveDown, MoveTo, MoveToColumn, RestorePosition, SavePosition};
use crossterm::queue;
use crossterm::style::{
    Attribute, Color as CrosstermColor, Colors, Print, SetAttribute, SetBackgroundColor, SetColors,
    SetForegroundColor,
};
use crossterm::terminal::{Clear, ClearType};
use ratatui::backend::Backend;
use ratatui::style::{Color, Modifier};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::terminal::custom_terminal::Terminal;

pub(crate) fn insert_history_lines<B>(
    terminal: &mut Terminal<B>,
    lines: Vec<Line<'static>>,
) -> Result<()>
where
    B: Backend + Write,
{
    if lines.is_empty() {
        return Ok(());
    }

    let screen_size = terminal.backend().size()?;
    let mut area = terminal.viewport_area;
    let mut should_update_area = false;
    let last_cursor_pos = terminal.last_known_cursor_pos;
    let wrap_width = area.width.max(1) as usize;
    let lines = wrap_history_lines(lines, wrap_width);
    let wrapped_rows = lines.len() as u16;
    if wrapped_rows == 0 {
        return Ok(());
    }

    let writer = terminal.backend_mut();
    let cursor_top = if area.bottom() < screen_size.height {
        let scroll_amount = wrapped_rows.min(screen_size.height - area.bottom());
        let top_1based = area.top() + 1;
        queue!(writer, SetScrollRegion(top_1based..screen_size.height))?;
        queue!(writer, MoveTo(0, area.top()))?;
        for _ in 0..scroll_amount {
            queue!(writer, Print("\x1bM"))?;
        }
        queue!(writer, ResetScrollRegion)?;

        let cursor_top = area.top().saturating_sub(1);
        area.y += scroll_amount;
        should_update_area = true;
        cursor_top
    } else {
        area.top().saturating_sub(1)
    };

    queue!(writer, SetScrollRegion(1..area.top()))?;
    queue!(writer, MoveTo(0, cursor_top))?;
    for line in &lines {
        queue!(writer, Print("\r\n"))?;
        write_history_line(writer, line, wrap_width)?;
    }
    queue!(writer, ResetScrollRegion)?;
    queue!(writer, MoveTo(last_cursor_pos.x, last_cursor_pos.y))?;
    std::io::Write::flush(writer)?;

    if should_update_area {
        terminal.set_viewport_area(area);
    }
    terminal.note_history_rows_inserted(wrapped_rows);
    Ok(())
}

fn wrap_history_lines(lines: Vec<Line<'static>>, wrap_width: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .flat_map(|line| wrap_history_line(line, wrap_width.max(1)))
        .collect()
}

fn wrap_history_line(line: Line<'static>, wrap_width: usize) -> Vec<Line<'static>> {
    if line.width() <= wrap_width {
        return vec![line];
    }

    let line_style = line.style;
    let mut rows = Vec::new();
    let mut row_spans = Vec::new();
    let mut row_width = 0usize;

    for span in line.spans {
        let span_style = span.style;
        let mut chunk = String::new();
        for ch in span.content.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if row_width > 0 && row_width + ch_width > wrap_width {
                if !chunk.is_empty() {
                    row_spans.push(Span::styled(std::mem::take(&mut chunk), span_style));
                }
                rows.push(line_from_spans(std::mem::take(&mut row_spans), line_style));
                row_width = 0;
            }
            chunk.push(ch);
            row_width = row_width.saturating_add(ch_width);
        }
        if !chunk.is_empty() {
            row_spans.push(Span::styled(chunk, span_style));
        }
    }

    if !row_spans.is_empty() {
        rows.push(line_from_spans(row_spans, line_style));
    }
    if rows.is_empty() {
        rows.push(line_from_spans(Vec::new(), line_style));
    }
    rows
}

fn line_from_spans(spans: Vec<Span<'static>>, style: ratatui::style::Style) -> Line<'static> {
    let mut line = Line::from(spans);
    line.style = style;
    line
}

fn write_history_line<W: Write>(writer: &mut W, line: &Line, wrap_width: usize) -> io::Result<()> {
    let physical_rows = line.width().max(1).div_ceil(wrap_width) as u16;
    if physical_rows > 1 {
        queue!(writer, SavePosition)?;
        for _ in 1..physical_rows {
            queue!(writer, MoveDown(1), MoveToColumn(0))?;
            queue!(writer, Clear(ClearType::UntilNewLine))?;
        }
        queue!(writer, RestorePosition)?;
    }
    queue!(
        writer,
        SetColors(Colors::new(
            line.style
                .fg
                .map(Into::into)
                .unwrap_or(CrosstermColor::Reset),
            line.style
                .bg
                .map(Into::into)
                .unwrap_or(CrosstermColor::Reset)
        )),
        Clear(ClearType::UntilNewLine)
    )?;
    write_spans(
        writer,
        line.spans.iter().map(|span| Span {
            style: span.style.patch(line.style),
            content: span.content.clone(),
        }),
    )
}

fn write_spans<'a, W, I>(writer: &mut W, spans: I) -> io::Result<()>
where
    W: Write,
    I: IntoIterator<Item = Span<'a>>,
{
    let mut fg = Color::Reset;
    let mut bg = Color::Reset;
    let mut modifier = Modifier::empty();
    for span in spans {
        let mut next_modifier = Modifier::empty();
        next_modifier.insert(span.style.add_modifier);
        next_modifier.remove(span.style.sub_modifier);
        if next_modifier != modifier {
            queue_modifier_diff(writer, modifier, next_modifier)?;
            modifier = next_modifier;
        }
        let next_fg = span.style.fg.unwrap_or(Color::Reset);
        let next_bg = span.style.bg.unwrap_or(Color::Reset);
        if next_fg != fg || next_bg != bg {
            queue!(
                writer,
                SetColors(Colors::new(next_fg.into(), next_bg.into()))
            )?;
            fg = next_fg;
            bg = next_bg;
        }
        queue!(writer, Print(span.content))?;
    }
    queue!(
        writer,
        SetForegroundColor(CrosstermColor::Reset),
        SetBackgroundColor(CrosstermColor::Reset),
        SetAttribute(Attribute::Reset)
    )
}

fn queue_modifier_diff<W: Write>(writer: &mut W, from: Modifier, to: Modifier) -> io::Result<()> {
    let removed = from - to;
    if removed.contains(Modifier::REVERSED) {
        queue!(writer, SetAttribute(Attribute::NoReverse))?;
    }
    if removed.contains(Modifier::BOLD) {
        queue!(writer, SetAttribute(Attribute::NormalIntensity))?;
        if to.contains(Modifier::DIM) {
            queue!(writer, SetAttribute(Attribute::Dim))?;
        }
    }
    if removed.contains(Modifier::ITALIC) {
        queue!(writer, SetAttribute(Attribute::NoItalic))?;
    }
    if removed.contains(Modifier::UNDERLINED) {
        queue!(writer, SetAttribute(Attribute::NoUnderline))?;
    }
    if removed.contains(Modifier::DIM) {
        queue!(writer, SetAttribute(Attribute::NormalIntensity))?;
    }
    if removed.contains(Modifier::CROSSED_OUT) {
        queue!(writer, SetAttribute(Attribute::NotCrossedOut))?;
    }

    let added = to - from;
    if added.contains(Modifier::REVERSED) {
        queue!(writer, SetAttribute(Attribute::Reverse))?;
    }
    if added.contains(Modifier::BOLD) {
        queue!(writer, SetAttribute(Attribute::Bold))?;
    }
    if added.contains(Modifier::ITALIC) {
        queue!(writer, SetAttribute(Attribute::Italic))?;
    }
    if added.contains(Modifier::UNDERLINED) {
        queue!(writer, SetAttribute(Attribute::Underlined))?;
    }
    if added.contains(Modifier::DIM) {
        queue!(writer, SetAttribute(Attribute::Dim))?;
    }
    if added.contains(Modifier::CROSSED_OUT) {
        queue!(writer, SetAttribute(Attribute::CrossedOut))?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetScrollRegion(std::ops::Range<u16>);

impl crossterm::Command for SetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{};{}r", self.0.start, self.0.end)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other("SetScrollRegion requires ANSI execution"))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResetScrollRegion;

impl crossterm::Command for ResetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[r")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "ResetScrollRegion requires ANSI execution",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::wrap_history_line;
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};

    #[test]
    fn wraps_long_history_line_before_terminal_insert() {
        let line = Line::from(vec![Span::styled(
            "Get-PSDrive -PSProvider FileSystem",
            Style::default().fg(Color::Blue),
        )]);

        let wrapped = wrap_history_line(line, 10);

        assert!(wrapped.len() > 1);
        assert!(wrapped.iter().all(|line| line.width() <= 10));
        assert_eq!(
            wrapped
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "Get-PSDrive -PSProvider FileSystem"
        );
    }

    #[test]
    fn preserves_wide_character_boundaries() {
        let line = Line::raw("磁盘分区占用率");

        let wrapped = wrap_history_line(line, 6);

        assert!(wrapped.len() > 1);
        assert!(wrapped.iter().all(|line| line.width() <= 6));
    }
}
