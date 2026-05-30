use crate::app::TuiApp;
use crate::app::core::transcript_owner::TranscriptOwner;
use crate::terminal::HistoryProjection;
use crate::terminal::HistoryUpdate;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::PreparedHistoryUpdate;
use crate::terminal::TerminalGuard;
use crate::terminal::prepare_history_lines;
use crate::terminal::prepare_history_tail_lines;
use crate::ui::chat_surface::ChatSurface;
use anyhow::Result;
use ratatui::layout::Rect;

#[derive(Default)]
pub(crate) struct TerminalProjectionController {
    reflow: TranscriptReflowState,
}

#[derive(Default)]
struct TranscriptReflowState {
    replay_requested: bool,
    replay_requested_during_stream: bool,
    pending_replay_reason: ReplayReason,
    last_replay_reason: ReplayReason,
    last_observed_width: Option<u16>,
    last_observed_viewport_height: Option<u16>,
    last_replayed_width: Option<u16>,
    last_replayed_viewport_height: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ReplayReason {
    #[default]
    None,
    Explicit,
    Metrics,
}

impl TerminalProjectionController {
    pub(crate) fn request_history_replay(&mut self) {
        self.reflow.request_replay();
    }

    pub(crate) fn reset(&mut self) {
        self.reflow.reset();
    }

    pub(crate) fn on_stream_boundary(&mut self) {
        self.reflow.on_stream_boundary();
    }

    pub(crate) fn build_plan(
        &mut self,
        transcript_owner: &mut TranscriptOwner,
        viewport_height: u16,
        terminal_width: u16,
        has_active_stream: bool,
        visible_history_rows: u16,
    ) -> HistoryProjection {
        let should_replay =
            self.reflow
                .begin_frame(terminal_width, viewport_height, has_active_stream);

        let history_update = if should_replay {
            let committed = transcript_owner.committed_history_cells();
            transcript_owner.mark_committed_history_replayed();
            if visible_history_rows > 0 && self.reflow.replay_reason() == ReplayReason::Metrics {
                HistoryUpdate::ReplayTail {
                    cells: committed,
                    max_rows: visible_history_rows as usize,
                }
            } else {
                HistoryUpdate::ReplayAll(committed)
            }
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
        let has_active_stream = app.transcript_owner.active_turn_id().is_some();
        let plan = self.build_plan(
            &mut app.transcript_owner,
            viewport_height,
            area.width,
            has_active_stream,
            terminal.terminal.visible_history_rows(),
        );
        let prepared = self.prepare_projection(plan, terminal.terminal.visible_history_rows() > 0);
        terminal.draw_projection(prepared, |frame| app.render(frame))?;
        Ok(())
    }

    fn prepare_projection(
        &self,
        projection: HistoryProjection,
        has_existing_history: bool,
    ) -> PreparedHistoryProjection {
        let HistoryProjection {
            viewport_height,
            history_render_width,
            history_update,
        } = projection;

        let history_update = match history_update {
            HistoryUpdate::ReplayAll(cells) => PreparedHistoryUpdate::ReplayAll(
                prepare_history_lines(cells, history_render_width, false),
            ),
            HistoryUpdate::ReplayTail { cells, max_rows } => PreparedHistoryUpdate::ReplayAll(
                prepare_history_tail_lines(cells, history_render_width, max_rows),
            ),
            HistoryUpdate::AppendTail(cells) => PreparedHistoryUpdate::AppendTail(
                prepare_history_lines(cells, history_render_width, has_existing_history),
            ),
        };

        PreparedHistoryProjection {
            viewport_height,
            history_update,
        }
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

impl TranscriptReflowState {
    fn request_replay(&mut self) {
        self.replay_requested = true;
        self.pending_replay_reason = ReplayReason::Explicit;
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn on_stream_boundary(&mut self) {
        if self.replay_requested_during_stream {
            self.replay_requested = true;
            self.replay_requested_during_stream = false;
            self.pending_replay_reason = ReplayReason::Metrics;
        }
    }

    fn replay_reason(&self) -> ReplayReason {
        self.last_replay_reason
    }

    fn begin_frame(&mut self, width: u16, viewport_height: u16, has_active_stream: bool) -> bool {
        let replay_metrics_changed = self.last_replayed_width != Some(width)
            || self.last_replayed_viewport_height != Some(viewport_height);
        let observed_metrics_changed = self.last_observed_width != Some(width)
            || self.last_observed_viewport_height != Some(viewport_height);

        self.last_observed_width = Some(width);
        self.last_observed_viewport_height = Some(viewport_height);

        if has_active_stream && observed_metrics_changed {
            self.replay_requested_during_stream = true;
        }

        if self.replay_requested || (!has_active_stream && replay_metrics_changed) {
            self.last_replay_reason = if self.replay_requested {
                self.pending_replay_reason
            } else {
                ReplayReason::Metrics
            };
            self.replay_requested = false;
            self.pending_replay_reason = ReplayReason::None;
            self.replay_requested_during_stream = false;
            self.last_replayed_width = Some(width);
            self.last_replayed_viewport_height = Some(viewport_height);
            true
        } else {
            self.last_replay_reason = ReplayReason::None;
            false
        }
    }
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

        let plan = controller.build_plan(&mut transcript_owner, 5, 80, false, 0);

        match plan.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::ReplayTail { .. } => panic!("explicit replay should replay all"),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }
    }

    #[test]
    fn metrics_change_with_empty_committed_history_still_replays_all() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, 80, false, 0);
        match first.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::ReplayTail { .. } => {
                panic!("initial replay without visible rows should replay all")
            }
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }

        let second = controller.build_plan(&mut transcript_owner, 6, 80, false, 0);
        match second.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::ReplayTail { .. } => {
                panic!("metrics replay without visible rows should replay all")
            }
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }
    }

    #[test]
    fn metrics_change_with_visible_history_replays_tail() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, 80, false, 0);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));

        let second = controller.build_plan(&mut transcript_owner, 6, 80, false, 4);
        match second.history_update {
            HistoryUpdate::ReplayTail { cells, max_rows } => {
                assert!(cells.is_empty());
                assert_eq!(max_rows, 4);
            }
            HistoryUpdate::ReplayAll(_) => {
                panic!("metrics replay should cap to visible history rows")
            }
            HistoryUpdate::AppendTail(_) => panic!("expected replay after metrics changed"),
        }
    }

    #[test]
    fn resize_during_stream_defers_replay_until_stream_boundary() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, 80, true, 0);
        match first.history_update {
            HistoryUpdate::AppendTail(cells) => assert!(cells.is_empty()),
            HistoryUpdate::ReplayAll(_) => panic!("streaming frame should defer replay"),
            HistoryUpdate::ReplayTail { .. } => panic!("streaming frame should defer replay"),
        }

        controller.on_stream_boundary();
        let second = controller.build_plan(&mut transcript_owner, 5, 80, false, 0);
        match second.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::ReplayTail { .. } => {
                panic!("stream-boundary replay without visible rows should replay all")
            }
            HistoryUpdate::AppendTail(_) => panic!("expected replay after stream boundary"),
        }
    }
}
