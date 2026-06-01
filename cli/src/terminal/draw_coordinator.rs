use anyhow::Result;
use ratatui::backend::Backend;
use std::io::Write;

use crate::terminal::Frame;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::PreparedHistoryUpdate;
use crate::terminal::custom_terminal::Terminal;
use crate::terminal::history_flush_queue::HistoryFlushQueue;
use crate::terminal::insert_history_lines_raw;
use crate::terminal::repaint_history_tail_raw;

pub(crate) struct DrawCoordinator<'a, B>
where
    B: Backend + Write,
{
    terminal: &'a mut Terminal<B>,
    history_flush_queue: &'a mut HistoryFlushQueue,
}

impl<'a, B> DrawCoordinator<'a, B>
where
    B: Backend + Write,
{
    pub(crate) fn new(
        terminal: &'a mut Terminal<B>,
        history_flush_queue: &'a mut HistoryFlushQueue,
    ) -> Self {
        Self {
            terminal,
            history_flush_queue,
        }
    }

    pub(crate) fn draw_frame(
        &mut self,
        projection: PreparedHistoryProjection,
        render: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        let PreparedHistoryProjection {
            viewport_height,
            history_update,
        } = projection;

        match history_update {
            PreparedHistoryUpdate::ReplayAll {
                cells,
                render_metrics,
                max_rows,
            } => {
                self.history_flush_queue
                    .replace_with_replay(cells, render_metrics, max_rows);
                self.terminal.clear_scrollback_and_visible_screen_ansi()?;
                self.terminal.ensure_viewport_height(viewport_height)?;
                insert_history_lines_raw(self.terminal, self.history_flush_queue.pending_lines())?;
                self.history_flush_queue.mark_flushed();
            }
            PreparedHistoryUpdate::AppendTail {
                cells,
                render_metrics,
            } => {
                self.history_flush_queue.append_tail(
                    cells,
                    render_metrics,
                    self.terminal.visible_history_rows() > 0,
                );
                self.terminal.ensure_viewport_height(viewport_height)?;
                insert_history_lines_raw(self.terminal, self.history_flush_queue.pending_lines())?;
                self.history_flush_queue.mark_flushed();
            }
            PreparedHistoryUpdate::ReflowVisibleTail {
                cells,
                render_metrics,
                max_rows,
            } => {
                self.history_flush_queue
                    .replace_with_replay(cells, render_metrics, Some(max_rows));
                self.terminal.ensure_viewport_height(viewport_height)?;
                repaint_history_tail_raw(self.terminal, self.history_flush_queue.pending_lines())?;
                self.history_flush_queue.mark_flushed();
            }
        }
        self.terminal.draw(render)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::DrawCoordinator;
    use crate::terminal::color_compat::{BackgroundTone, ColorDepth, TerminalCapabilities};
    use crate::terminal::custom_terminal::Terminal;
    use crate::terminal::history_flush_queue::HistoryFlushQueue;
    use crate::terminal::{HistoryRenderMetrics, PreparedHistoryProjection, PreparedHistoryUpdate};
    use crate::ui::widgets::history_cell::{HistoryCell, HistoryFormat, HistoryTone};
    use ratatui::backend::{Backend, WindowSize};
    use ratatui::buffer::Cell;
    use ratatui::layout::{Position, Size};
    use std::io;
    use std::io::Write;

    #[derive(Debug)]
    struct RecordingBackend {
        size: Size,
        cursor: Position,
        bytes: Vec<u8>,
        fail_writes: bool,
    }

    impl RecordingBackend {
        fn new(width: u16, height: u16) -> Self {
            Self {
                size: Size { width, height },
                cursor: Position {
                    x: 0,
                    y: height.saturating_sub(1),
                },
                bytes: Vec::new(),
                fail_writes: false,
            }
        }

        fn output(&self) -> String {
            String::from_utf8_lossy(&self.bytes).into_owned()
        }
    }

    impl Write for RecordingBackend {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.fail_writes {
                return Err(io::Error::other("injected write failure"));
            }
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.fail_writes {
                return Err(io::Error::other("injected flush failure"));
            }
            Ok(())
        }
    }

    impl Backend for RecordingBackend {
        fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
        where
            I: Iterator<Item = (u16, u16, &'a Cell)>,
        {
            for (_x, _y, cell) in content {
                self.write_all(cell.symbol().as_bytes())?;
            }
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
                pixels: Size {
                    width: self.size.width,
                    height: self.size.height,
                },
            })
        }

        fn flush(&mut self) -> io::Result<()> {
            Write::flush(self)
        }
    }

    fn projection(update: PreparedHistoryUpdate) -> PreparedHistoryProjection {
        PreparedHistoryProjection {
            viewport_height: 4,
            history_update: update,
        }
    }

    fn projection_with_height(
        viewport_height: u16,
        update: PreparedHistoryUpdate,
    ) -> PreparedHistoryProjection {
        PreparedHistoryProjection {
            viewport_height,
            history_update: update,
        }
    }

    fn metrics(width: usize) -> HistoryRenderMetrics {
        HistoryRenderMetrics {
            width,
            left_padding: 0,
        }
    }

    fn test_capabilities() -> TerminalCapabilities {
        TerminalCapabilities {
            color_depth: ColorDepth::NoColor,
            supports_synchronized_update: false,
            background_tone: BackgroundTone::Unknown,
        }
    }

    #[test]
    fn append_tail_flush_failure_keeps_pending_lines_for_next_frame() {
        let backend = RecordingBackend::new(80, 12);
        let mut terminal = Terminal::new(backend, test_capabilities()).expect("terminal");
        let mut queue = HistoryFlushQueue::default();

        terminal.backend_mut().fail_writes = true;
        let failed = DrawCoordinator::new(&mut terminal, &mut queue).draw_frame(
            projection(PreparedHistoryUpdate::AppendTail {
                cells: vec![HistoryCell::agent(
                    "cloudagent",
                    "visible prefix and final suffix",
                    HistoryFormat::Markdown,
                )],
                render_metrics: metrics(80),
            }),
            |_frame| {},
        );
        assert!(failed.is_err());
        assert!(!queue.pending_lines().is_empty());

        terminal.backend_mut().fail_writes = false;
        DrawCoordinator::new(&mut terminal, &mut queue)
            .draw_frame(
                projection(PreparedHistoryUpdate::AppendTail {
                    cells: vec![HistoryCell::agent(
                        "cloudagent",
                        "next message",
                        HistoryFormat::Markdown,
                    )],
                    render_metrics: metrics(80),
                }),
                |_frame| {},
            )
            .expect("retry draw succeeds");

        let output = terminal.backend().output();
        assert!(output.contains("visible prefix and final suffix"));
        assert!(output.contains("next message"));
        assert!(queue.pending_lines().is_empty());
    }

    #[test]
    fn command_append_then_agent_append_preserves_order_and_agent_tail() {
        let backend = RecordingBackend::new(100, 18);
        let mut terminal = Terminal::new(backend, test_capabilities()).expect("terminal");
        let mut queue = HistoryFlushQueue::default();

        DrawCoordinator::new(&mut terminal, &mut queue)
            .draw_frame(
                projection(PreparedHistoryUpdate::AppendTail {
                    cells: vec![HistoryCell::exec(
                        "Run command",
                        "cargo check -p cli",
                        Some("running @ D:/learn/gifti/cloudagent".to_string()),
                        HistoryTone::Control,
                    )],
                    render_metrics: metrics(100),
                }),
                |_frame| {},
            )
            .expect("command draw succeeds");

        DrawCoordinator::new(&mut terminal, &mut queue)
            .draw_frame(
                projection(PreparedHistoryUpdate::AppendTail {
                    cells: vec![HistoryCell::agent(
                        "cloudagent",
                        "已改成真实生效的时间戳命名。\n\n最后一句不能丢。",
                        HistoryFormat::Markdown,
                    )],
                    render_metrics: metrics(100),
                }),
                |_frame| {},
            )
            .expect("agent draw succeeds");

        let output = terminal.backend().output();
        let command_pos = output.find("Run command").expect("command rendered");
        let agent_pos = output
            .find("已改成真实生效的时间戳命名")
            .expect("agent rendered");

        assert!(command_pos < agent_pos);
        assert!(output.contains("最后一句不能丢。"));
    }

    #[test]
    fn command_stream_and_final_replay_keep_agent_tail_after_resize() {
        let backend = RecordingBackend::new(100, 20);
        let mut terminal = Terminal::new(backend, test_capabilities()).expect("terminal");
        let mut queue = HistoryFlushQueue::default();

        let command_cell = HistoryCell::exec(
            "Run command",
            "cargo test -p cli terminal::draw_coordinator::tests:: -- --nocapture",
            Some("completed (exit 0) @ D:/learn/gifti/cloudagent".to_string()),
            HistoryTone::Control,
        );

        DrawCoordinator::new(&mut terminal, &mut queue)
            .draw_frame(
                projection_with_height(
                    6,
                    PreparedHistoryUpdate::AppendTail {
                        cells: vec![command_cell.clone()],
                        render_metrics: metrics(100),
                    },
                ),
                |_frame| {},
            )
            .expect("command draw succeeds");

        let mut stream_head =
            HistoryCell::agent("cloudagent", "第一段流式回复，", HistoryFormat::Markdown);
        stream_head.set_provisional_stream(true);
        let stream_tail = HistoryCell::agent(
            "cloudagent",
            "第二段包含最终尾巴：TAIL-END",
            HistoryFormat::Markdown,
        )
        .with_stream_continuation(true)
        .with_provisional_stream(true);

        DrawCoordinator::new(&mut terminal, &mut queue)
            .draw_frame(
                projection_with_height(
                    6,
                    PreparedHistoryUpdate::AppendTail {
                        cells: vec![stream_head, stream_tail],
                        render_metrics: metrics(100),
                    },
                ),
                |_frame| {},
            )
            .expect("stream draw succeeds");

        terminal.backend_mut().size = Size {
            width: 88,
            height: 16,
        };
        let final_agent = HistoryCell::agent(
            "cloudagent",
            "第一段流式回复，第二段包含最终尾巴：TAIL-END\n\n最终完成句也不能丢。",
            HistoryFormat::Markdown,
        );

        DrawCoordinator::new(&mut terminal, &mut queue)
            .draw_frame(
                projection_with_height(
                    5,
                    PreparedHistoryUpdate::ReplayAll {
                        cells: vec![command_cell, final_agent],
                        render_metrics: metrics(88),
                        max_rows: None,
                    },
                ),
                |_frame| {},
            )
            .expect("final replay succeeds");

        let output = terminal.backend().output();
        let command_pos = output.rfind("Run command").expect("command rendered");
        let final_pos = output
            .rfind("第一段流式回复")
            .expect("final agent rendered");

        assert!(command_pos < final_pos);
        assert!(output.contains("TAIL-END"));
        assert!(output.contains("最终完成句也不能丢。"));
        assert!(!output.contains("responding"));
        assert!(queue.pending_lines().is_empty());
    }

    #[test]
    fn viewport_shrink_repaints_full_visible_history_tail_after_clear() {
        let backend = RecordingBackend::new(100, 18);
        let mut terminal = Terminal::new(backend, test_capabilities()).expect("terminal");
        let mut queue = HistoryFlushQueue::default();
        let history = vec![
            HistoryCell::user("oldest message"),
            HistoryCell::agent("cloudagent", "middle message", HistoryFormat::Markdown),
            HistoryCell::agent("cloudagent", "latest visible tail", HistoryFormat::Markdown),
        ];

        DrawCoordinator::new(&mut terminal, &mut queue)
            .draw_frame(
                projection_with_height(
                    8,
                    PreparedHistoryUpdate::ReplayAll {
                        cells: history.clone(),
                        render_metrics: metrics(100),
                        max_rows: None,
                    },
                ),
                |_frame| {},
            )
            .expect("initial replay succeeds");

        DrawCoordinator::new(&mut terminal, &mut queue)
            .draw_frame(
                projection_with_height(
                    4,
                    PreparedHistoryUpdate::ReflowVisibleTail {
                        cells: history,
                        render_metrics: metrics(100),
                        max_rows: 14,
                    },
                ),
                |_frame| {},
            )
            .expect("viewport shrink repair succeeds");

        let output = terminal.backend().output();
        assert!(output.contains("\u{1b}[1;1H\u{1b}[K"));
        assert!(output.contains("latest visible tail"));
        assert!(output.contains("oldest message"));
        assert!(output.contains("middle message"));
        assert!(queue.pending_lines().is_empty());
    }
}
