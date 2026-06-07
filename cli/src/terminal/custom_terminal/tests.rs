use super::{DrawCommand, Terminal, diff_buffers, draw_updates};
use crate::terminal::color_compat::{BackgroundTone, ColorDepth, TerminalCapabilities};
use ratatui::backend::{Backend, WindowSize};
use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Rect, Size};
use ratatui::style::Color;
use std::io;
use std::io::Write;
use std::str;

#[derive(Debug)]
struct TestBackend {
    size: Size,
    cursor: Position,
    bytes: Vec<u8>,
}

impl TestBackend {
    fn new(width: u16, height: u16) -> Self {
        Self {
            size: Size { width, height },
            cursor: Position {
                x: 0,
                y: height.saturating_sub(1),
            },
            bytes: Vec::new(),
        }
    }
}

impl Write for TestBackend {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Backend for TestBackend {
    fn draw<'a, I>(&mut self, _content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.cursor = position.into();
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: self.size,
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        Write::flush(self)
    }
}

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

#[test]
fn bottom_aligned_viewport_stays_bottom_aligned_when_height_shrinks() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(
        backend,
        TerminalCapabilities {
            color_depth: ColorDepth::NoColor,
            supports_synchronized_update: false,
            background_tone: BackgroundTone::Unknown,
        },
    )
    .expect("terminal");

    terminal.ensure_viewport_height(12).expect("initial height");
    assert_eq!(terminal.viewport_area, Rect::new(0, 18, 100, 12));

    terminal.ensure_viewport_height(6).expect("shrunk height");
    assert_eq!(terminal.viewport_area, Rect::new(0, 24, 100, 6));
}
