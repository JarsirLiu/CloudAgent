use crate::app::TuiApp;
use crate::app::core::transcript_owner::TranscriptOwner;
use crate::terminal::HistoryProjection;
use crate::terminal::HistoryRepair;
use crate::terminal::HistoryUpdate;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::PreparedHistoryUpdate;
use crate::terminal::TerminalGuard;
use crate::terminal::prepare_history_lines;
use crate::terminal::prepare_history_tail_lines;
use crate::ui::chat_surface::ChatSurface;
use anyhow::Result;
use ratatui::layout::{Rect, Size};

#[derive(Default)]
pub(crate) struct TerminalProjectionController {
    reflow: TranscriptReflowState,
    reflow_policy: ReflowPolicy,
    last_viewport_height: Option<u16>,
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
        _history_capacity_rows: u16,
    ) -> HistoryProjection {
        let viewport_shrank = self
            .last_viewport_height
            .is_some_and(|previous| viewport_height < previous);
        self.last_viewport_height = Some(viewport_height);

        let should_replay = self.reflow.begin_frame(terminal_size, has_active_stream);

        let history_update = if should_replay {
            let committed = transcript_owner.committed_history_cells();
            transcript_owner.mark_committed_history_replayed();
            HistoryUpdate::ReplayAll(committed)
        } else {
            HistoryUpdate::AppendTail(transcript_owner.drain_pending_history_cells())
        };
        let history_capacity_rows = terminal_size.height.saturating_sub(viewport_height) as usize;
        let history_repair = if !should_replay && viewport_shrank && history_capacity_rows > 0 {
            Some(HistoryRepair {
                cells: transcript_owner.committed_history_cells(),
                max_rows: history_capacity_rows,
            })
        } else {
            None
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
            history_repair,
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
        let history_capacity_rows = area.height.saturating_sub(viewport_height);
        let has_active_stream = app.transcript_owner.active_turn_id().is_some();
        let plan = self.build_plan(
            &mut app.transcript_owner,
            viewport_height,
            size,
            has_active_stream,
            history_capacity_rows,
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
            history_repair,
        } = projection;

        let history_update = match history_update {
            HistoryUpdate::ReplayAll(cells) => {
                let lines = if let Some(max_rows) = self.reflow_policy.max_rows {
                    prepare_history_tail_lines(cells, history_render_width, max_rows)
                } else {
                    prepare_history_lines(cells, history_render_width, false)
                };
                PreparedHistoryUpdate::ReplayAll(lines)
            }
            HistoryUpdate::AppendTail(cells) => PreparedHistoryUpdate::AppendTail(
                prepare_history_lines(cells, history_render_width, has_existing_history),
            ),
        };
        let history_repair_tail = history_repair
            .map(|repair| {
                prepare_history_tail_lines(repair.cells, history_render_width, repair.max_rows)
            })
            .unwrap_or_default();

        PreparedHistoryProjection {
            viewport_height,
            history_update,
            history_repair_tail,
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
}

#[cfg(test)]
mod tests {
    use super::{ReflowPolicy, TerminalProjectionController};
    use crate::app::core::transcript_owner::TranscriptOwner;
    use crate::terminal::{HistoryProjection, HistoryRepair, HistoryUpdate, PreparedHistoryUpdate};
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

        let plan = controller.build_plan(&mut transcript_owner, 5, size(80, 24), false, 0);

        match plan.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }
    }

    #[test]
    fn terminal_metrics_change_with_empty_committed_history_still_replays_all() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, size(80, 24), false, 0);
        match first.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }

        let second = controller.build_plan(&mut transcript_owner, 5, size(100, 24), false, 0);
        match second.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }
    }

    #[test]
    fn terminal_metrics_change_replays_all_history_even_with_visible_capacity() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, size(80, 24), false, 0);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));

        let second = controller.build_plan(&mut transcript_owner, 5, size(80, 30), false, 4);
        match second.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected replay after metrics changed"),
        }
    }

    #[test]
    fn viewport_height_change_does_not_replay_history() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, size(80, 24), false, 0);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));

        let second = controller.build_plan(&mut transcript_owner, 11, size(80, 24), false, 0);
        match second.history_update {
            HistoryUpdate::AppendTail(cells) => assert!(cells.is_empty()),
            HistoryUpdate::ReplayAll(_) => panic!("viewport-only layout changes must not replay"),
        }
    }

    #[test]
    fn resize_during_stream_replays_now_and_repairs_at_stream_boundary() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, size(80, 24), true, 0);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));

        let resized = controller.build_plan(&mut transcript_owner, 5, size(100, 24), true, 0);
        match resized.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("streaming resize should replay now"),
        }

        controller.on_stream_boundary();
        let repaired = controller.build_plan(&mut transcript_owner, 5, size(100, 24), false, 0);
        match repaired.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("expected final repair after stream boundary"),
        }
    }

    #[test]
    fn first_stream_frame_does_not_request_boundary_repair_without_resize() {
        let mut controller = TerminalProjectionController::default();
        let mut transcript_owner = TranscriptOwner::default();

        let first = controller.build_plan(&mut transcript_owner, 5, size(80, 24), true, 0);
        match first.history_update {
            HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
            HistoryUpdate::AppendTail(_) => panic!("first frame still establishes scrollback"),
        }

        controller.on_stream_boundary();
        let second = controller.build_plan(&mut transcript_owner, 5, size(80, 24), false, 0);
        match second.history_update {
            HistoryUpdate::AppendTail(cells) => assert!(cells.is_empty()),
            HistoryUpdate::ReplayAll(_) => panic!("no resize happened during stream"),
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

        let first = controller.build_plan(&mut transcript_owner, 12, size(80, 24), false, 0);
        assert!(matches!(first.history_update, HistoryUpdate::ReplayAll(_)));
        assert!(first.history_repair.is_none());

        let second = controller.build_plan(&mut transcript_owner, 6, size(80, 24), false, 0);
        match second.history_update {
            HistoryUpdate::AppendTail(cells) => assert!(cells.is_empty()),
            HistoryUpdate::ReplayAll(_) => panic!("viewport shrink should repair, not replay"),
        }
        let repair = second.history_repair.expect("expected tail repair");
        assert_eq!(repair.max_rows, 18);
        assert_eq!(repair.cells.len(), 3);
    }

    #[test]
    fn reflow_policy_caps_prepared_replay_lines_without_changing_plan() {
        let mut controller = TerminalProjectionController::default();
        controller.set_reflow_policy(ReflowPolicy { max_rows: Some(3) });

        let prepared = controller.prepare_projection(
            HistoryProjection {
                viewport_height: 5,
                history_render_width: 80,
                history_update: HistoryUpdate::ReplayAll(vec![
                    HistoryCell::user("oldest message"),
                    HistoryCell::user("middle message"),
                    HistoryCell::user("latest message"),
                ]),
                history_repair: None,
            },
            false,
        );

        match prepared.history_update {
            PreparedHistoryUpdate::ReplayAll(lines) => {
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
            PreparedHistoryUpdate::AppendTail(_) => panic!("expected replay-all path"),
        }
    }

    #[test]
    fn prepare_projection_caps_history_repair_tail() {
        let controller = TerminalProjectionController::default();

        let prepared = controller.prepare_projection(
            HistoryProjection {
                viewport_height: 5,
                history_render_width: 80,
                history_update: HistoryUpdate::AppendTail(Vec::new()),
                history_repair: Some(HistoryRepair {
                    cells: vec![
                        HistoryCell::user("oldest message"),
                        HistoryCell::user("middle message"),
                        HistoryCell::user("latest message"),
                    ],
                    max_rows: 3,
                }),
            },
            true,
        );

        let rendered = prepared
            .history_repair_tail
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(prepared.history_repair_tail.len() <= 3);
        assert!(rendered.contains("latest message"));
        assert!(!rendered.contains("oldest message"));
    }
}
