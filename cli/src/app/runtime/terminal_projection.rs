use crate::app::TuiApp;
use crate::app::core::transcript_owner::TranscriptOwner;
use crate::terminal::HistoryProjection;
use crate::terminal::HistoryUpdate;
use crate::terminal::TerminalGuard;
use crate::ui::chat_surface::ChatSurface;
use anyhow::Result;
use ratatui::layout::Rect;

#[derive(Default)]
pub(crate) struct TerminalProjectionController {
    history_replay_required: bool,
    last_history_replay_width: Option<u16>,
    last_history_replay_viewport_height: Option<u16>,
}

impl TerminalProjectionController {
    pub(crate) fn request_history_replay(&mut self) {
        self.history_replay_required = true;
    }

    pub(crate) fn reset(&mut self) {
        self.history_replay_required = false;
        self.last_history_replay_width = None;
        self.last_history_replay_viewport_height = None;
    }

    pub(crate) fn build_plan(
        &mut self,
        transcript_owner: &mut TranscriptOwner,
        viewport_height: u16,
        terminal_width: u16,
    ) -> HistoryProjection {
        let replay_metrics_changed =
            self.last_history_replay_width != Some(terminal_width)
                || self.last_history_replay_viewport_height != Some(viewport_height);
        let history_update = if self.history_replay_required || replay_metrics_changed {
            self.history_replay_required = false;
            self.last_history_replay_width = Some(terminal_width);
            self.last_history_replay_viewport_height = Some(viewport_height);
            let committed = transcript_owner.committed_history_cells();
            transcript_owner.mark_committed_history_replayed();
            HistoryUpdate::ReplayAll(committed)
        } else {
            HistoryUpdate::AppendTail(transcript_owner.drain_pending_history_cells())
        };

        HistoryProjection {
            viewport_height,
            history_render_width: ChatSurface::render_width_for_area(Rect::new(
                0,
                0,
                terminal_width,
                viewport_height.max(1),
            )),
            history_update,
        }
    }

    pub(crate) fn draw_frame(
        &mut self,
        app: &mut TuiApp,
        terminal: &mut TerminalGuard,
    ) -> Result<()> {
        let size = terminal.terminal.size()?;
        let area = Rect::new(0, 0, size.width, size.height);
        let viewport_height = ChatSurface::desired_viewport_height(app, area);
        let plan = self.build_plan(&mut app.transcript_owner, viewport_height, area.width);
        terminal.draw_projection(plan, |frame| app.render(frame))?;
        Ok(())
    }
}

pub(crate) fn draw_with_terminal_projection(
    app: &mut TuiApp,
    terminal: &mut TerminalGuard,
) -> Result<()> {
    let mut projection = std::mem::take(&mut app.terminal_projection);
    let result = projection.draw_frame(app, terminal);
    app.terminal_projection = projection;
    result
}

#[cfg(test)]
mod tests {
    use super::TerminalProjectionController;
    use crate::app::core::transcript_owner::TranscriptOwner;
    use crate::terminal::HistoryUpdate;

    #[test]
    fn explicit_replay_with_empty_committed_history_still_replays_all() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();
        controller.request_history_replay();

        let plan = controller.build_plan(&mut transcript_owner, 5, 80);

        match plan.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }
    }

    #[test]
    fn metrics_change_with_empty_committed_history_still_replays_all() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, 80);
        match first.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }

        let second = controller.build_plan(&mut transcript_owner, 6, 80);
        match second.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }
    }
}
