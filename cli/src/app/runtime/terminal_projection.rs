use crate::app::TuiApp;
use crate::app::core::transcript_owner::TranscriptOwner;
use crate::terminal::HistoryProjection;
use crate::terminal::HistoryUpdate;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::PreparedHistoryUpdate;
use crate::terminal::TerminalGuard;
use crate::ui::chat_surface::ChatSurface;
use anyhow::Result;
use ratatui::layout::{Rect, Size};

#[derive(Default)]
pub(crate) struct TerminalProjectionController {
    reflow: TranscriptReflowState,
    reflow_policy: ReflowPolicy,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReflowPolicy {
    pub(crate) max_rows: Option<usize>,
}

#[derive(Default)]
struct TranscriptReflowState {
    replay_requested: bool,
    replay_requested_during_stream: bool,
    pending_replay_reason: ReplayReason,
    last_replay_reason: ReplayReason,
    last_observed_terminal_size: Option<Size>,
    last_replayed_terminal_size: Option<Size>,
    last_viewport_height: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ReplayReason {
    #[default]
    None,
    Explicit,
    Metrics,
}

impl TerminalProjectionController {
    pub(crate) fn set_reflow_policy(&mut self, policy: ReflowPolicy) {
        self.reflow_policy = policy;
    }

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
        terminal_size: Size,
        has_active_stream: bool,
    ) -> HistoryProjection {
        let transcript_requested_replay = transcript_owner.take_history_replay_requested();
        let should_replay = self.reflow.begin_frame(terminal_size, has_active_stream)
            || transcript_requested_replay;

        let visible_tail_reflow_rows =
            self.reflow
                .visible_tail_reflow_rows(viewport_height, terminal_size, should_replay);
        if has_active_stream && visible_tail_reflow_rows.is_some() {
            self.reflow.mark_reflowed_during_stream();
        }

        let history_update = if should_replay {
            let committed = transcript_owner.committed_history_cells();
            transcript_owner.mark_committed_history_replayed();
            HistoryUpdate::ReplayAll(committed)
        } else if let Some(max_rows) = visible_tail_reflow_rows {
            let committed = transcript_owner.committed_history_cells();
            transcript_owner.mark_committed_history_replayed();
            HistoryUpdate::ReflowVisibleTail {
                cells: committed,
                max_rows,
            }
        } else {
            HistoryUpdate::AppendTail(transcript_owner.drain_pending_history_cells())
        };

        HistoryProjection {
            viewport_height,
            history_render_width: ChatSurface::render_width_for_area(Rect::new(
                0,
                0,
                terminal_size.width,
                terminal_size.height.max(1),
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
            size,
            has_active_stream,
        );
        let prepared = self.prepare_projection(plan);
        terminal.draw_projection(prepared, |frame| app.render(frame))?;
        Ok(())
    }

    fn prepare_projection(&self, projection: HistoryProjection) -> PreparedHistoryProjection {
        let HistoryProjection {
            viewport_height,
            history_render_width,
            history_update,
        } = projection;

        let history_update = match history_update {
            HistoryUpdate::ReplayAll(cells) => PreparedHistoryUpdate::ReplayAll {
                cells,
                render_width: history_render_width,
                max_rows: self.reflow_policy.max_rows,
            },
            HistoryUpdate::AppendTail(cells) => PreparedHistoryUpdate::AppendTail {
                cells,
                render_width: history_render_width,
            },
            HistoryUpdate::ReflowVisibleTail { cells, max_rows } => {
                PreparedHistoryUpdate::ReflowVisibleTail {
                    cells,
                    render_width: history_render_width,
                    max_rows,
                }
            }
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

    fn mark_reflowed_during_stream(&mut self) {
        self.replay_requested_during_stream = true;
    }

    fn begin_frame(&mut self, terminal_size: Size, has_active_stream: bool) -> bool {
        let replay_metrics_changed = self.last_replayed_terminal_size != Some(terminal_size);
        let observed_metrics_changed = self
            .last_observed_terminal_size
            .is_some_and(|previous| previous != terminal_size);

        self.last_observed_terminal_size = Some(terminal_size);

        if has_active_stream && observed_metrics_changed {
            self.replay_requested_during_stream = true;
        }

        if self.replay_requested || replay_metrics_changed {
            self.last_replay_reason = if self.replay_requested {
                self.pending_replay_reason
            } else {
                ReplayReason::Metrics
            };
            self.replay_requested = false;
            self.pending_replay_reason = ReplayReason::None;
            self.last_replayed_terminal_size = Some(terminal_size);
            true
        } else {
            self.last_replay_reason = ReplayReason::None;
            false
        }
    }

    fn visible_tail_reflow_rows(
        &mut self,
        viewport_height: u16,
        terminal_size: Size,
        replaying_all: bool,
    ) -> Option<usize> {
        let previous = self.last_viewport_height.replace(viewport_height);
        if replaying_all || previous.is_none_or(|previous| viewport_height >= previous) {
            return None;
        }
        let history_capacity = terminal_size.height.saturating_sub(viewport_height) as usize;
        (history_capacity > 0).then_some(history_capacity)
    }
}

#[cfg(test)]
mod tests {
    use super::{ReflowPolicy, TerminalProjectionController};
    use crate::app::core::transcript_owner::TranscriptOwner;
    use crate::terminal::{HistoryProjection, HistoryUpdate, PreparedHistoryUpdate};
    use crate::ui::widgets::history_cell::HistoryCell;
    use ratatui::layout::Size;

    fn size(width: u16, height: u16) -> Size {
        Size { width, height }
    }

    #[test]
    fn explicit_replay_with_empty_committed_history_still_replays_all() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();
        controller.request_history_replay();

        let plan = controller.build_plan(&mut transcript_owner, 5, size(80, 24), false);

        match plan.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
            HistoryUpdate::ReflowVisibleTail { .. } => panic!("expected replay-all path"),
        }
    }

    #[test]
    fn terminal_metrics_change_with_empty_committed_history_still_replays_all() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, size(80, 24), false);
        match first.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
            HistoryUpdate::ReflowVisibleTail { .. } => panic!("expected replay-all path"),
        }

        let second = controller.build_plan(&mut transcript_owner, 5, size(100, 24), false);
        match second.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
            HistoryUpdate::ReflowVisibleTail { .. } => panic!("expected replay-all path"),
        }
    }

    #[test]
    fn terminal_metrics_change_replays_all_history_even_with_visible_capacity() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, size(80, 24), false);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));

        let second = controller.build_plan(&mut transcript_owner, 5, size(80, 30), false);
        match second.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay after metrics changed"),
            HistoryUpdate::ReflowVisibleTail { .. } => {
                panic!("expected replay after metrics changed")
            }
        }
    }

    #[test]
    fn viewport_expand_does_not_replay_or_repair_history() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, size(80, 24), false);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));

        let second = controller.build_plan(&mut transcript_owner, 11, size(80, 24), false);
        match second.history_update {
            HistoryUpdate::AppendTail(cells) => assert!(cells.is_empty()),
            HistoryUpdate::ReplayAll(_) => panic!("viewport-only layout changes must not replay"),
            HistoryUpdate::ReflowVisibleTail { .. } => {
                panic!("viewport expand must not reflow visible tail")
            }
        }
    }

    #[test]
    fn resize_during_stream_replays_now_and_repairs_at_stream_boundary() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, size(80, 24), true);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));

        let resized = controller.build_plan(&mut transcript_owner, 5, size(100, 24), true);
        match resized.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("streaming resize should replay now"),
            HistoryUpdate::ReflowVisibleTail { .. } => panic!("streaming resize should replay now"),
        }

        controller.on_stream_boundary();
        let repaired = controller.build_plan(&mut transcript_owner, 5, size(100, 24), false);
        match repaired.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected final repair after stream boundary"),
            HistoryUpdate::ReflowVisibleTail { .. } => {
                panic!("expected final repair after stream boundary")
            }
        }
    }

    #[test]
    fn first_stream_frame_does_not_request_boundary_repair_without_resize() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, size(80, 24), true);
        match first.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("first frame still establishes scrollback"),
            HistoryUpdate::ReflowVisibleTail { .. } => {
                panic!("first frame still establishes scrollback")
            }
        }

        controller.on_stream_boundary();
        let second = controller.build_plan(&mut transcript_owner, 5, size(80, 24), false);
        match second.history_update {
            HistoryUpdate::AppendTail(cells) => assert!(cells.is_empty()),
            HistoryUpdate::ReplayAll(_) => panic!("no resize happened during stream"),
            HistoryUpdate::ReflowVisibleTail { .. } => panic!("no resize happened during stream"),
        }
    }

    #[test]
    fn viewport_shrink_repairs_visible_history_tail_without_replay() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();
        transcript_owner.queue_history_cells_for_test(vec![
            HistoryCell::user("oldest message"),
            HistoryCell::user("middle message"),
            HistoryCell::user("latest message"),
        ]);

        let first = controller.build_plan(&mut transcript_owner, 12, size(80, 24), false);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));

        let second = controller.build_plan(&mut transcript_owner, 6, size(80, 24), false);
        match second.history_update {
            HistoryUpdate::ReflowVisibleTail { cells, max_rows } => {
                assert_eq!(cells.len(), 3);
                assert_eq!(max_rows, 18);
            }
            HistoryUpdate::AppendTail(_) => panic!("viewport shrink should reflow visible tail"),
            HistoryUpdate::ReplayAll(_) => panic!("viewport shrink should reflow, not replay all"),
        }
    }

    #[test]
    fn viewport_reflow_during_stream_repairs_at_stream_boundary() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();
        transcript_owner.queue_history_cells_for_test(vec![
            HistoryCell::user("oldest message"),
            HistoryCell::user("middle message"),
            HistoryCell::user("latest message"),
        ]);

        let first = controller.build_plan(&mut transcript_owner, 12, size(80, 24), true);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));

        let shrunk = controller.build_plan(&mut transcript_owner, 6, size(80, 24), true);
        match shrunk.history_update {
            HistoryUpdate::ReflowVisibleTail { cells, max_rows } => {
                assert_eq!(cells.len(), 3);
                assert_eq!(max_rows, 18);
            }
            HistoryUpdate::AppendTail(_) => panic!("stream viewport shrink should reflow tail"),
            HistoryUpdate::ReplayAll(_) => panic!("stream viewport shrink should not replay all"),
        }

        controller.on_stream_boundary();
        let repaired = controller.build_plan(&mut transcript_owner, 6, size(80, 24), false);
        match repaired.history_update {
            HistoryUpdate::ReplayAll(cells) => assert_eq!(cells.len(), 3),
            HistoryUpdate::AppendTail(_) => panic!("expected stream-boundary replay after reflow"),
            HistoryUpdate::ReflowVisibleTail { .. } => {
                panic!("stream-boundary repair should replay canonical history")
            }
        }
    }

    #[test]
    fn reflow_policy_is_carried_to_draw_layer_without_rendering_lines() {
        let mut controller = TerminalProjectionController::default();
        controller.set_reflow_policy(ReflowPolicy { max_rows: Some(3) });

        let prepared = controller.prepare_projection(HistoryProjection {
            viewport_height: 5,
            history_render_width: 80,
            history_update: HistoryUpdate::ReplayAll(vec![
                HistoryCell::user("oldest message"),
                HistoryCell::user("middle message"),
                HistoryCell::user("latest message"),
            ]),
        });

        match prepared.history_update {
            PreparedHistoryUpdate::ReplayAll {
                cells,
                render_width,
                max_rows,
            } => {
                assert_eq!(cells.len(), 3);
                assert_eq!(render_width, 80);
                assert_eq!(max_rows, Some(3));
            }
            PreparedHistoryUpdate::AppendTail { .. } => panic!("expected replay-all path"),
            PreparedHistoryUpdate::ReflowVisibleTail { .. } => panic!("expected replay-all path"),
        }
    }
}
