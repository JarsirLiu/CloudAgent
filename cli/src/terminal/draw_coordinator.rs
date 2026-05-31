use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use std::io;

use crate::terminal::Frame;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::PreparedHistoryUpdate;
use crate::terminal::custom_terminal::Terminal;
use crate::terminal::insert_history_lines_raw;
use crate::terminal::repaint_visible_history_tail;

pub(crate) struct DrawCoordinator<'a> {
    terminal: &'a mut Terminal<CrosstermBackend<io::Stdout>>,
}

impl<'a> DrawCoordinator<'a> {
    pub(crate) fn new(terminal: &'a mut Terminal<CrosstermBackend<io::Stdout>>) -> Self {
        Self { terminal }
    }

    pub(crate) fn draw_frame(
        &mut self,
        projection: PreparedHistoryProjection,
        render: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        let PreparedHistoryProjection {
            viewport_height,
            history_update,
            history_repair_tail,
        } = projection;

        match history_update {
            PreparedHistoryUpdate::ReplayAll(committed_history) => {
                self.terminal.clear_scrollback_and_visible_screen_ansi()?;
                self.terminal.ensure_viewport_height(viewport_height)?;
                insert_history_lines_raw(self.terminal, committed_history)?;
            }
            PreparedHistoryUpdate::AppendTail(committed_tail) => {
                // The first transition out of the welcome/fullscreen state still needs the
                // viewport established before any history insert. After the history region
                // exists, append committed cells against the current stable boundary first,
                // then adjust the active viewport.
                if self.terminal.viewport_area.top() == 0
                    || should_resize_before_append(self.terminal.viewport_area, viewport_height)
                {
                    self.terminal.ensure_viewport_height(viewport_height)?;
                    insert_history_lines_raw(self.terminal, committed_tail)?;
                } else {
                    insert_history_lines_raw(self.terminal, committed_tail)?;
                    self.terminal.ensure_viewport_height(viewport_height)?;
                }
            }
        }
        repaint_visible_history_tail(self.terminal, history_repair_tail)?;
        self.terminal.draw(render)?;
        Ok(())
    }
}

fn should_resize_before_append(current_viewport: Rect, next_viewport_height: u16) -> bool {
    current_viewport.height > 0 && next_viewport_height < current_viewport.height
}

#[cfg(test)]
mod tests {
    use super::should_resize_before_append;
    use ratatui::layout::Rect;

    #[test]
    fn shrinking_viewport_resizes_before_appending_history() {
        assert!(should_resize_before_append(Rect::new(0, 10, 80, 12), 6));
    }

    #[test]
    fn growing_or_equal_viewport_keeps_append_then_resize_order() {
        assert!(!should_resize_before_append(Rect::new(0, 10, 80, 12), 12));
        assert!(!should_resize_before_append(Rect::new(0, 10, 80, 12), 16));
    }
}
