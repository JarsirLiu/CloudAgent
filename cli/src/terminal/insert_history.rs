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

use crate::terminal::color_compat::{TerminalCapabilities, adapt_bg, adapt_color};
use crate::terminal::custom_terminal::Terminal;
use crate::ui::widgets::history_cell::HistoryCell;

pub(crate) fn prepare_history_lines(
    cells: Vec<HistoryCell>,
    render_width: usize,
    has_existing_history: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut has_emitted_history = has_existing_history;

    for cell in cells {
        let mut display = cell.to_lines_with_mode(render_width);
        if display.is_empty() {
            continue;
        }
        if has_emitted_history && !cell.is_stream_continuation() {
            lines.push(Line::from(""));
        }
        lines.append(&mut display);
        has_emitted_history = true;
    }

    lines
}

#[cfg(test)]
fn prepare_history_tail_lines(
    cells: Vec<HistoryCell>,
    render_width: usize,
    max_rows: usize,
) -> Vec<Line<'static>> {
    if max_rows == 0 || cells.is_empty() {
        return Vec::new();
    }

    let mut selected = Vec::new();
    let mut selected_rows = 0usize;
    let mut has_newer_cell = false;

    for cell in cells.into_iter().rev() {
        let display_rows = cell.to_lines_with_mode(render_width).len();
        if display_rows == 0 {
            continue;
        }
        let separator_rows = usize::from(has_newer_cell && !cell.is_stream_continuation());
        selected_rows = selected_rows.saturating_add(display_rows + separator_rows);
        selected.push(cell);
        has_newer_cell = true;
        if selected_rows >= max_rows {
            break;
        }
    }

    selected.reverse();
    let mut lines = prepare_history_lines(selected, render_width, false);
    if lines.len() > max_rows {
        lines.drain(0..lines.len() - max_rows);
    }
    lines
}

pub(crate) fn insert_history_lines_raw<B>(
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
    let mut history_region_was_empty = area.top() == 0;
    let last_cursor_pos = terminal.last_known_cursor_pos;
    let capabilities = terminal.capabilities();
    let wrap_width = area.width.max(1) as usize;
    let lines = wrap_history_lines(lines, wrap_width);
    let wrapped_rows = lines.len() as u16;
    if wrapped_rows == 0 {
        return Ok(());
    }

    let writer = terminal.backend_mut();
    let cursor_top = if area.top() == 0 {
        let viewport_height = area.height.max(1).min(screen_size.height.max(1));
        let max_reserved_rows = screen_size.height.saturating_sub(viewport_height);
        let reserved_rows = wrapped_rows.min(max_reserved_rows);

        if reserved_rows > 0 {
            queue!(writer, MoveTo(0, screen_size.height.saturating_sub(1)))?;
            for _ in 0..reserved_rows {
                queue!(writer, Print("\n"))?;
            }
            area.y = reserved_rows;
            should_update_area = true;
        }
        0
    } else if area.bottom() < screen_size.height {
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

    let has_history_region = area.top() > 0;
    if has_history_region {
        queue!(writer, SetScrollRegion(1..area.top()))?;
    }
    queue!(writer, MoveTo(0, cursor_top))?;
    for (index, line) in lines.iter().enumerate() {
        if index > 0 || !history_region_was_empty {
            queue!(writer, Print("\r\n"))?;
        }
        write_history_line(writer, line, wrap_width, capabilities)?;
        history_region_was_empty = false;
    }
    if has_history_region {
        queue!(writer, ResetScrollRegion)?;
    }
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

fn write_history_line<W: Write>(
    writer: &mut W,
    line: &Line,
    wrap_width: usize,
    capabilities: TerminalCapabilities,
) -> io::Result<()> {
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
                .map(|color| adapt_color(color, capabilities))
                .map(Into::into)
                .unwrap_or(CrosstermColor::Reset),
            line.style
                .bg
                .map(|color| adapt_bg(color, capabilities))
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
        capabilities,
    )
}

fn write_spans<'a, W, I>(
    writer: &mut W,
    spans: I,
    capabilities: TerminalCapabilities,
) -> io::Result<()>
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
        let next_fg = adapt_color(span.style.fg.unwrap_or(Color::Reset), capabilities);
        let next_bg = adapt_bg(span.style.bg.unwrap_or(Color::Reset), capabilities);
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
    use super::{
        prepare_history_lines, prepare_history_tail_lines, wrap_history_line, write_history_line,
    };
    use crate::terminal::color_compat::{BackgroundTone, ColorDepth, TerminalCapabilities};
    use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};
    use std::str;

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

    #[test]
    fn continuation_cells_do_not_insert_blank_separator() {
        let first = HistoryCell::reasoning("Reasoning", "thinking");
        let mut second = HistoryCell::info("Run command", "rg cli", HistoryTone::Control);
        second.set_stream_continuation(true);

        let lines = prepare_history_lines(vec![first, second], 80, false);
        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(rendered.iter().filter(|line| line.is_empty()).count(), 0);
    }

    #[test]
    fn new_non_continuation_cell_inserts_blank_separator() {
        let first = HistoryCell::user("hello");
        let second = HistoryCell::reasoning("Reasoning", "thinking");

        let lines = prepare_history_lines(vec![first, second], 80, false);
        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(rendered.iter().any(|line| line.is_empty()));
    }

    #[test]
    fn tail_history_lines_are_capped_to_latest_rows() {
        let lines = prepare_history_tail_lines(
            vec![
                HistoryCell::user("oldest message"),
                HistoryCell::user("middle message"),
                HistoryCell::user("latest message"),
            ],
            80,
            3,
        );
        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(lines.len() <= 3);
        assert!(rendered.contains("latest message"));
        assert!(!rendered.contains("oldest message"));
    }

    #[test]
    fn write_history_line_downgrades_truecolor_output_for_ansi256() {
        let line = Line::from(vec![Span::styled(
            "hello",
            Style::default().fg(Color::Rgb(120, 170, 255)),
        )]);
        let mut bytes = Vec::new();

        write_history_line(
            &mut bytes,
            &line,
            80,
            TerminalCapabilities {
                color_depth: ColorDepth::Ansi256,
                supports_synchronized_update: true,
                background_tone: BackgroundTone::Dark,
            },
        )
        .unwrap();

        let output = str::from_utf8(&bytes).unwrap();
        assert!(!output.contains("38;2;"));
    }
}
