use std::fmt;
use std::io;
use std::io::Write;

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::{
    Attribute, Color as CrosstermColor, Colors, Print, SetAttribute, SetBackgroundColor, SetColors,
    SetForegroundColor,
};
use crossterm::terminal::{Clear, ClearType as CrosstermClearType};
use ratatui::backend::Backend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect, Size};
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Widget;

use crate::terminal::color_compat::{TerminalCapabilities, adapt_bg, adapt_color};
use crate::text_width::display_width;

pub(crate) struct Frame<'a> {
    cursor_position: Option<Position>,
    viewport_area: Rect,
    buffer: &'a mut Buffer,
}

impl Frame<'_> {
    pub(crate) const fn area(&self) -> Rect {
        self.viewport_area
    }

    pub(crate) fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) {
        widget.render(area, self.buffer);
    }

    pub(crate) fn set_cursor_position<P: Into<Position>>(&mut self, position: P) {
        self.cursor_position = Some(position.into());
    }
}

pub(crate) struct Terminal<B>
where
    B: Backend + Write,
{
    backend: B,
    buffers: [Buffer; 2],
    current: usize,
    hidden_cursor: bool,
    pub(crate) viewport_area: Rect,
    last_known_screen_size: Size,
    pub(crate) last_known_cursor_pos: Position,
    visible_history_rows: u16,
    capabilities: TerminalCapabilities,
}

impl<B> Terminal<B>
where
    B: Backend + Write,
{
    pub(crate) fn new(mut backend: B, capabilities: TerminalCapabilities) -> io::Result<Self> {
        let screen_size = backend.size()?;
        let cursor_pos = backend.get_cursor_position().unwrap_or(Position {
            x: 0,
            y: screen_size.height.saturating_sub(1),
        });
        Ok(Self {
            backend,
            buffers: [Buffer::empty(Rect::ZERO), Buffer::empty(Rect::ZERO)],
            current: 0,
            hidden_cursor: false,
            viewport_area: Rect::new(0, cursor_pos.y, 0, 0),
            last_known_screen_size: screen_size,
            last_known_cursor_pos: cursor_pos,
            visible_history_rows: 0,
            capabilities,
        })
    }

    pub(crate) const fn backend(&self) -> &B {
        &self.backend
    }

    pub(crate) fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub(crate) fn size(&self) -> io::Result<Size> {
        self.backend.size()
    }

    pub(crate) fn set_viewport_area(&mut self, area: Rect) {
        self.current_buffer_mut().resize(area);
        self.previous_buffer_mut().resize(area);
        self.viewport_area = area;
        self.visible_history_rows = self.visible_history_rows.min(area.top());
    }

    pub(crate) fn resize(&mut self, screen_size: Size) {
        self.last_known_screen_size = screen_size;
    }

    fn autoresize(&mut self) -> io::Result<()> {
        let screen_size = self.size()?;
        if screen_size != self.last_known_screen_size {
            self.resize(screen_size);
        }
        Ok(())
    }

    fn current_buffer(&self) -> &Buffer {
        &self.buffers[self.current]
    }

    fn current_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.current]
    }

    fn previous_buffer(&self) -> &Buffer {
        &self.buffers[1 - self.current]
    }

    fn previous_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[1 - self.current]
    }

    fn get_frame(&mut self) -> Frame<'_> {
        Frame {
            cursor_position: None,
            viewport_area: self.viewport_area,
            buffer: self.current_buffer_mut(),
        }
    }

    pub(crate) fn draw(&mut self, render_callback: impl FnOnce(&mut Frame)) -> io::Result<()> {
        self.autoresize()?;
        let mut frame = self.get_frame();
        render_callback(&mut frame);
        let cursor_position = frame.cursor_position;
        self.flush()?;
        match cursor_position {
            Some(position) => {
                self.show_cursor()?;
                self.set_cursor_position(position)?;
            }
            None => self.hide_cursor()?,
        }
        self.swap_buffers();
        Backend::flush(&mut self.backend)?;
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        let updates = diff_buffers(self.previous_buffer(), self.current_buffer());
        let last_put = updates.iter().rfind(|command| command.is_put());
        if let Some(DrawCommand::Put { x, y, .. }) = last_put {
            self.last_known_cursor_pos = Position { x: *x, y: *y };
        }
        draw_updates(&mut self.backend, updates.into_iter(), self.capabilities)
    }

    pub(crate) fn clear_rows(&mut self, start_y: u16, end_y_exclusive: u16) -> io::Result<()> {
        if start_y >= end_y_exclusive {
            return Ok(());
        }
        for y in start_y..end_y_exclusive {
            queue!(
                self.backend,
                MoveTo(0, y),
                Clear(CrosstermClearType::UntilNewLine)
            )?;
        }
        self.previous_buffer_mut().reset();
        std::io::Write::flush(&mut self.backend)?;
        Ok(())
    }

    pub(crate) fn clear_scrollback_and_visible_screen_ansi(&mut self) -> io::Result<()> {
        queue!(
            self.backend,
            SetAttribute(Attribute::Reset),
            SetForegroundColor(CrosstermColor::Reset),
            SetBackgroundColor(CrosstermColor::Reset),
            Print("\x1b[r"),
            Print("\x1b[H"),
            Clear(CrosstermClearType::All),
            Print("\x1b[3J"),
            MoveTo(0, 0)
        )?;
        self.previous_buffer_mut().reset();
        self.current_buffer_mut().reset();
        self.visible_history_rows = 0;
        self.viewport_area = Rect::ZERO;
        std::io::Write::flush(&mut self.backend)?;
        Ok(())
    }

    pub(crate) fn note_history_rows_inserted(&mut self, inserted_rows: u16) {
        self.visible_history_rows = self
            .visible_history_rows
            .saturating_add(inserted_rows)
            .min(self.viewport_area.top());
    }

    pub(crate) fn visible_history_rows(&self) -> u16 {
        self.visible_history_rows
    }

    pub(crate) fn capabilities(&self) -> TerminalCapabilities {
        self.capabilities
    }

    fn swap_buffers(&mut self) {
        self.previous_buffer_mut().reset();
        self.current = 1 - self.current;
    }

    pub(crate) fn hide_cursor(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.hidden_cursor = true;
        Ok(())
    }

    pub(crate) fn show_cursor(&mut self) -> io::Result<()> {
        self.backend.show_cursor()?;
        self.hidden_cursor = false;
        Ok(())
    }

    pub(crate) fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let position = position.into();
        self.backend.set_cursor_position(position)?;
        self.last_known_cursor_pos = position;
        Ok(())
    }

    pub(crate) fn ensure_viewport_height(&mut self, height: u16) -> Result<()> {
        let size = self.size()?;
        let terminal_height_shrank = size.height < self.last_known_screen_size.height;
        let terminal_height_grew = size.height > self.last_known_screen_size.height;
        let viewport_was_bottom_aligned =
            self.viewport_area.bottom() == self.last_known_screen_size.height;
        let previous = self.viewport_area;
        let mut area = previous;
        area.height = height.clamp(1, size.height.max(1));
        area.width = size.width;

        self.resize(size);

        if previous == Rect::ZERO
            || previous.height == 0
            || (area.height != previous.height && viewport_was_bottom_aligned)
        {
            area.y = size.height.saturating_sub(area.height);
        }

        if area.y < previous.y {
            let grow_by = previous.y - area.y;
            if !terminal_height_shrank {
                self.scroll_region_up(0..previous.top(), grow_by)?;
            }
        }

        if area.bottom() > size.height {
            let scroll_by = area.bottom() - size.height;
            self.scroll_region_up(0..area.top(), scroll_by)?;
            area.y = size.height - area.height;
        } else if terminal_height_grew && viewport_was_bottom_aligned {
            area.y = size.height.saturating_sub(area.height);
        }

        if area != previous {
            self.set_viewport_area(area);
            self.clear_rows(previous.y.min(area.y), size.height)?;
        }
        Ok(())
    }

    fn scroll_region_up(&mut self, region: std::ops::Range<u16>, scroll_by: u16) -> Result<()> {
        if scroll_by == 0 || region.is_empty() {
            return Ok(());
        }
        let writer = self.backend_mut();
        queue!(writer, SetScrollRegion(region.start + 1..region.end))?;
        queue!(writer, MoveTo(0, region.end.saturating_sub(1)))?;
        for _ in 0..scroll_by {
            queue!(writer, Print("\n"))?;
        }
        queue!(writer, ResetScrollRegion)?;
        std::io::Write::flush(writer)?;
        Ok(())
    }
}

impl<B> Drop for Terminal<B>
where
    B: Backend + Write,
{
    fn drop(&mut self) {
        if self.hidden_cursor {
            let _ = self.show_cursor();
        }
    }
}

#[derive(Debug)]
enum DrawCommand {
    Put { x: u16, y: u16, cell: Cell },
    ClearToEnd { x: u16, y: u16, bg: Color },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetScrollRegion(std::ops::Range<u16>);

impl crossterm::Command for SetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{};{}r", self.0.start, self.0.end)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "SetScrollRegion requires ANSI execution",
        ))
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
    fn execute_winapi(&self) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "ResetScrollRegion requires ANSI execution",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

impl DrawCommand {
    fn is_put(&self) -> bool {
        matches!(self, DrawCommand::Put { .. })
    }
}

fn diff_buffers(previous: &Buffer, next: &Buffer) -> Vec<DrawCommand> {
    let mut updates = Vec::new();
    let mut last_nonblank_columns = vec![0; previous.area.height as usize];

    for y in 0..previous.area.height {
        let row_start = y as usize * previous.area.width as usize;
        let row_end = row_start + previous.area.width as usize;
        let row = &next.content[row_start..row_end];
        let bg = row.last().map(|cell| cell.bg).unwrap_or(Color::Reset);
        let mut last_nonblank_column = None;
        let mut column = 0usize;
        while column < row.len() {
            let cell = &row[column];
            let width = display_width(cell.symbol());
            if cell.symbol() != " " || cell.bg != bg || cell.modifier != Modifier::empty() {
                last_nonblank_column = Some(column + width.saturating_sub(1));
            }
            column += width.max(1);
        }
        let clear_start = last_nonblank_column.map_or(0, |column| column.saturating_add(1));
        if clear_start < row.len() {
            let (x, y) = previous.pos_of(row_start + clear_start);
            updates.push(DrawCommand::ClearToEnd { x, y, bg });
        }
        last_nonblank_columns[y as usize] = last_nonblank_column.unwrap_or(0) as u16;
    }

    let mut invalidated = 0usize;
    let mut to_skip = 0usize;
    for (idx, (next_cell, previous_cell)) in
        next.content.iter().zip(previous.content.iter()).enumerate()
    {
        if !next_cell.skip && (next_cell != previous_cell || invalidated > 0) && to_skip == 0 {
            let (x, y) = previous.pos_of(idx);
            let row = idx / previous.area.width as usize;
            if x <= last_nonblank_columns[row] {
                updates.push(DrawCommand::Put {
                    x,
                    y,
                    cell: next_cell.clone(),
                });
            }
        }
        to_skip = display_width(next_cell.symbol()).saturating_sub(1);
        let affected_width =
            display_width(next_cell.symbol()).max(display_width(previous_cell.symbol()));
        invalidated = affected_width.max(invalidated).saturating_sub(1);
    }
    updates
}

fn draw_updates(
    writer: &mut impl Write,
    commands: impl Iterator<Item = DrawCommand>,
    capabilities: TerminalCapabilities,
) -> io::Result<()> {
    let mut fg = Color::Reset;
    let mut bg = Color::Reset;
    let mut modifier = Modifier::empty();
    let mut last_pos: Option<Position> = None;

    for command in commands {
        let (x, y) = match command {
            DrawCommand::Put { x, y, .. } | DrawCommand::ClearToEnd { x, y, .. } => (x, y),
        };
        if !matches!(last_pos, Some(pos) if x == pos.x + 1 && y == pos.y) {
            queue!(writer, MoveTo(x, y))?;
        }
        last_pos = Some(Position { x, y });

        match command {
            DrawCommand::Put { cell, .. } => {
                let next_fg = adapt_color(cell.fg, capabilities);
                let next_bg = adapt_bg(cell.bg, capabilities);
                if cell.modifier != modifier {
                    queue_modifier_diff(writer, modifier, cell.modifier)?;
                    modifier = cell.modifier;
                }
                if next_fg != fg || next_bg != bg {
                    queue!(
                        writer,
                        SetColors(Colors::new(next_fg.into(), next_bg.into()))
                    )?;
                    fg = next_fg;
                    bg = next_bg;
                }
                queue!(writer, Print(cell.symbol()))?;
            }
            DrawCommand::ClearToEnd { bg: clear_bg, .. } => {
                let clear_bg = adapt_bg(clear_bg, capabilities);
                queue!(writer, SetAttribute(Attribute::Reset))?;
                modifier = Modifier::empty();
                queue!(writer, SetBackgroundColor(clear_bg.into()))?;
                bg = clear_bg;
                queue!(writer, Clear(CrosstermClearType::UntilNewLine))?;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{DrawCommand, diff_buffers, draw_updates};
    use crate::terminal::color_compat::{BackgroundTone, ColorDepth, TerminalCapabilities};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use std::str;

    #[test]
    fn diff_buffers_clears_fully_blank_row_from_first_column() {
        let area = Rect::new(0, 0, 4, 1);
        let mut previous = Buffer::empty(area);
        previous[(0, 0)].set_symbol("A");
        previous[(1, 0)].set_symbol("B");
        previous[(2, 0)].set_symbol("C");
        previous[(3, 0)].set_symbol("D");

        let next = Buffer::empty(area);
        let updates = diff_buffers(&previous, &next);

        assert!(matches!(
            updates.first(),
            Some(DrawCommand::ClearToEnd { x: 0, y: 0, .. })
        ));
    }

    #[test]
    fn draw_updates_downgrades_truecolor_output_for_ansi256() {
        let mut bytes = Vec::new();
        let mut cell = ratatui::buffer::Cell::default();
        cell.set_symbol("x");
        cell.set_fg(Color::Rgb(120, 170, 255));

        let command = DrawCommand::Put { x: 0, y: 0, cell };
        draw_updates(
            &mut bytes,
            [command].into_iter(),
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
